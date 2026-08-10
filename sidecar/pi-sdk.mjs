#!/usr/bin/env node

import { pathToFileURL } from "node:url";

const options = parseArgs(process.argv.slice(2));
if (!options.sdkEntry || !options.cwd) {
  throw new Error(
    "Usage: pi-sdk.mjs --sdk-entry <path> --cwd <path> [--session <path>]",
  );
}

const {
  createAgentSessionFromServices,
  createAgentSessionRuntime,
  createAgentSessionServices,
  getAgentDir,
  runRpcMode,
  SessionManager,
} = await import(pathToFileURL(options.sdkEntry).href);

const agentDir = getAgentDir();
const sessionManager = options.session
  ? SessionManager.open(options.session, undefined, options.cwd)
  : SessionManager.create(options.cwd);

const createRuntime = async ({
  cwd,
  agentDir,
  sessionManager,
  sessionStartEvent,
}) => {
  const services = await createAgentSessionServices({ cwd, agentDir });
  return {
    ...(await createAgentSessionFromServices({
      services,
      sessionManager,
      sessionStartEvent,
    })),
    services,
    diagnostics: services.diagnostics,
  };
};

const runtime = await createAgentSessionRuntime(createRuntime, {
  cwd: options.cwd,
  agentDir,
  sessionManager,
});

await runRpcMode(runtime);

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
