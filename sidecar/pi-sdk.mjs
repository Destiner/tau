#!/usr/bin/env node

import { pathToFileURL } from "node:url";

const options = parseArgs(process.argv.slice(2));
if (!options.sdkEntry || !options.cwd) {
  throw new Error(
    "Usage: pi-sdk.mjs --sdk-entry <path> --cwd <path> [--session <path>]",
  );
}

const { createAgentSession, ModelRuntime, SessionManager } = await import(
  pathToFileURL(options.sdkEntry).href
);
const modelRuntime = await ModelRuntime.create();
let session;
let unsubscribe;

await replaceSession(options.session);

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
  let newline;
  while ((newline = input.indexOf("\n")) >= 0) {
    let line = input.slice(0, newline);
    input = input.slice(newline + 1);
    if (line.endsWith("\r")) line = line.slice(0, -1);
    if (!line) continue;
    try {
      void handleCommand(JSON.parse(line));
    } catch (error) {
      process.stderr.write(`${errorMessage(error)}\n`);
    }
  }
});
process.stdin.resume();

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    disposeSession();
    process.exit(0);
  });
}

async function replaceSession(sessionPath) {
  disposeSession();
  const sessionManager = sessionPath
    ? SessionManager.open(sessionPath, undefined, options.cwd)
    : SessionManager.create(options.cwd);
  const result = await createAgentSession({
    cwd: options.cwd,
    modelRuntime,
    sessionManager,
  });
  session = result.session;
  unsubscribe = session.subscribe((event) => emit(event));
}

function disposeSession() {
  unsubscribe?.();
  unsubscribe = undefined;
  session?.dispose();
  session = undefined;
}

async function handleCommand(command) {
  const id = typeof command.id === "string" ? command.id : undefined;
  const type = typeof command.type === "string" ? command.type : "";
  try {
    switch (type) {
      case "prompt": {
        let answered = false;
        const answer = (accepted) => {
          if (answered) return;
          answered = true;
          if (accepted) respond(id, type);
          else reject(id, type, "Prompt rejected before it was accepted.");
        };
        void session
          .prompt(command.message, {
            images: command.images,
            streamingBehavior: command.streamingBehavior,
            preflightResult: answer,
          })
          .catch((error) => {
            if (!answered) reject(id, type, errorMessage(error));
            else emit({ type: "extension_error", error: errorMessage(error) });
          });
        return;
      }
      case "abort":
        await session.abort();
        respond(id, type);
        return;
      case "new_session":
        if (session.isStreaming)
          throw new Error("Cannot change sessions while Pi is streaming.");
        await replaceSession();
        respond(id, type, { cancelled: false });
        return;
      case "switch_session":
        if (session.isStreaming)
          throw new Error("Cannot change sessions while Pi is streaming.");
        await replaceSession(command.sessionPath);
        respond(id, type, { cancelled: false });
        return;
      case "get_state":
        respond(id, type, sessionState());
        return;
      case "get_messages":
        respond(id, type, { messages: session.messages });
        return;
      case "get_available_models":
        respond(id, type, { models: modelRuntime.getAvailableSnapshot() });
        return;
      case "get_commands":
        respond(id, type, { commands: availableCommands() });
        return;
      case "set_model": {
        const model = modelRuntime
          .getAvailableSnapshot()
          .find(
            (item) =>
              item.provider === command.provider && item.id === command.modelId,
          );
        if (!model)
          throw new Error(
            `Model not found: ${command.provider}/${command.modelId}`,
          );
        await session.setModel(model);
        respond(id, type, model);
        return;
      }
      case "get_available_thinking_levels":
        respond(id, type, { levels: session.getAvailableThinkingLevels() });
        return;
      case "set_thinking_level":
        session.setThinkingLevel(command.level);
        respond(id, type);
        return;
      default:
        reject(
          id,
          type || "unknown",
          `Unsupported SDK sidecar command: ${type || "unknown"}`,
        );
    }
  } catch (error) {
    reject(id, type || "unknown", errorMessage(error));
  }
}

function availableCommands() {
  const extensionCommands = session.extensionRunner
    .getRegisteredCommands()
    .map((command) => ({
      name: command.invocationName,
      description: command.description,
      source: "extension",
      sourceInfo: command.sourceInfo,
    }));
  const templates = session.promptTemplates.map((template) => ({
    name: template.name,
    description: template.description,
    source: "prompt",
    sourceInfo: template.sourceInfo,
  }));
  const skills = session.resourceLoader.getSkills().skills.map((skill) => ({
    name: `skill:${skill.name}`,
    description: skill.description,
    source: "skill",
    sourceInfo: skill.sourceInfo,
  }));
  return [...extensionCommands, ...templates, ...skills];
}

function sessionState() {
  return {
    model: session.model,
    thinkingLevel: session.thinkingLevel,
    isStreaming: session.isStreaming,
    isCompacting: session.isCompacting,
    steeringMode: session.steeringMode,
    followUpMode: session.followUpMode,
    sessionFile: session.sessionFile,
    sessionId: session.sessionId,
    sessionName: session.sessionName,
    autoCompactionEnabled: session.autoCompactionEnabled,
    messageCount: session.messages.length,
    pendingMessageCount: session.pendingMessageCount,
  };
}

function respond(id, command, data) {
  emit({
    id,
    type: "response",
    command,
    success: true,
    ...(data === undefined ? {} : { data }),
  });
}

function reject(id, command, message) {
  emit({ id, type: "response", command, success: false, error: { message } });
}

function emit(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (name === "--sdk-entry") result.sdkEntry = value;
    if (name === "--cwd") result.cwd = value;
    if (name === "--session") result.session = value;
  }
  return result;
}
