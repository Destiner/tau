/*
 * Pi protocol layer: the bridge events and RPC dispatch that drive a session
 * runtime, the extension UI requests they surface, and the controller
 * lifecycle (start, release, dispose). Mirrors src-tauri/src/pi.rs.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  appendLocalErrors,
  asRecord,
  hydrateTranscript,
  messageFailure,
  stringValue,
  toolArgumentsText,
  toolResultText,
  toolSummary,
} from "./transcript";
import { describePiError } from "./error";
import {
  abortAcknowledgeDelay,
  abortProbeTimers,
  activeController,
  activeExtensionDialog,
  clearActiveSession,
  commandOption,
  controllerByKey,
  controllerByRuntimeId,
  controllerHasPendingDialog,
  createPhantomSession,
  draftTitle,
  ephemeralSessionByController,
  extensionDialogTimeouts,
  extensionNotificationTimeouts,
  finishRemoteConnection,
  firstUserMessage,
  idleRuntimeLimit,
  isControllerSelected,
  nextRequestId,
  normalizeEffort,
  presentRemoteConnectionError,
  replacementProbeDelays,
  replacementProbeTimers,
  setActiveError,
  setControllerError,
  state,
  touchController,
  workspaceContainsSession,
  type EphemeralSession,
  type ExtensionDialog,
  type ExtensionDialogMethod,
  type ExtensionNotification,
  type ExtensionNotificationType,
  type ProjectSummary,
  type SessionController,
  type WorkspaceSnapshot,
} from "../../composables/state";
import type { ModelOption } from "./model-scope";
import type { CommandOption } from "../commands";
import type { PiBridgeEvent } from "./bridge";

export async function sendPhantomMessage(
  controller: SessionController,
  message: string,
  command: boolean,
) {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project || !controller.phantom) return;

  const optimisticId = `optimistic-user-${Date.now()}`;
  controller.pendingPrompt = {
    message,
    optimisticId,
    command,
    stateRequestId: "",
    messagesRequestId: "",
    selectedModelProvider: controller.currentModelProvider,
    selectedModelId: controller.currentModelId,
    selectedModelName: controller.currentModelName,
    selectedEffort: controller.currentEffort,
    settingsRequestId: "",
    settingsStep: "",
  };
  controller.draft = "";
  controller.working = true;
  if (!command) {
    controller.messages.push({ id: optimisticId, kind: "user", text: message });
  }
  controller.status = "";

  if (controller.ready && controller.generation) {
    controller.starting = true;
    const stateRequestId = nextRequestId("state");
    controller.pendingPrompt.stateRequestId = stateRequestId;
    await rpc(controller, { id: stateRequestId, type: "get_state" });
    return;
  }
  await startController(controller, project, undefined, true);
}

export async function startController(
  controller: SessionController,
  project: ProjectSummary,
  sessionPath?: string,
  preserveMessages = false,
) {
  if (controller.starting || controller.disposed) return;
  controller.ready = false;
  controller.starting = true;
  controller.stopping = false;
  controller.connectingRemote = Boolean(project.connectionString);
  controller.bootstrapStateRequestId = "";
  controller.bootstrapSessionPath = sessionPath ?? "";
  controller.runStateRequestId = "";
  controller.startMessagesRequestId = "";
  controller.commandPromptRequestId = "";
  controller.commandSyncRequestId = "";
  controller.replacementProbeRequestId = "";
  controller.abortProbeRequestId = "";
  touchController(controller);
  clearSessionReplacementWatch(controller);
  clearAbortWatch(controller);
  if (!preserveMessages && controller.messages.length === 0) {
    controller.messages = [];
  }
  controller.models = [];
  controller.efforts = [];
  controller.commands = [];
  controller.commandsLoaded = false;
  controller.status = "";
  discardControllerDialogs(controller);
  void refreshModelScope(controller, project);

  try {
    if (project.connectionString) {
      controller.generation = await invoke<number>("start_pi_remote", {
        runtimeId: controller.runtimeId,
        connectionString: project.connectionString,
        workingDirectory: project.workingDirectory,
        sessionPath: sessionPath ?? null,
      });
    } else {
      controller.generation = await invoke<number>("start_pi", {
        runtimeId: controller.runtimeId,
        projectPath: project.workingDirectory,
        sessionPath: sessionPath ?? null,
      });
    }
    if (controller.disposed) {
      await invoke("stop_pi", { runtimeId: controller.runtimeId });
      return;
    }
    await requestBootstrap(controller);
  } catch (error) {
    controller.starting = false;
    controller.connectingRemote = false;
    controller.syncing = false;
    if (project.connectionString)
      presentRemoteConnectionError(controller, error);
    else setControllerError(controller, error);
    if (controller.pendingPrompt) cancelPendingPrompt(controller, error);
  }
}

export async function refreshModelScope(
  controller: SessionController,
  project: ProjectSummary,
) {
  try {
    const patterns = project.connectionString
      ? await invoke<string[]>("read_remote_model_scope", {
          connectionString: project.connectionString,
        })
      : await invoke<string[]>("read_model_scope");
    if (controller.disposed || !Array.isArray(patterns)) return;
    controller.modelScope = patterns.filter(
      (pattern) => typeof pattern === "string",
    );
  } catch {
    controller.modelScope = [];
  }
}

export async function requestBootstrap(controller: SessionController) {
  await rpc(controller, {
    id: nextRequestId("models"),
    type: "get_available_models",
  });
  await rpc(controller, {
    id: nextRequestId("commands"),
    type: "get_commands",
  });
  const stateRequestId = nextRequestId("state");
  controller.bootstrapStateRequestId = stateRequestId;
  if (controller.pendingPrompt) {
    controller.pendingPrompt.stateRequestId = stateRequestId;
  }
  await rpc(controller, { id: stateRequestId, type: "get_state" });
}

export async function rpc(
  controller: SessionController,
  request: Record<string, unknown>,
) {
  await invoke("send_pi", { runtimeId: controller.runtimeId, request });
}

export async function submitExtensionDialog(value: string | boolean) {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  const response =
    dialog.method === "confirm" && typeof value === "boolean"
      ? {
          type: "extension_ui_response",
          id: dialog.requestId,
          confirmed: value,
        }
      : typeof value === "string"
        ? { type: "extension_ui_response", id: dialog.requestId, value }
        : {
            type: "extension_ui_response",
            id: dialog.requestId,
            cancelled: true,
          };
  await respondToExtensionDialog(dialog, response);
}

export async function cancelExtensionDialog() {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  await respondToExtensionDialog(dialog, {
    type: "extension_ui_response",
    id: dialog.requestId,
    cancelled: true,
  });
}

export async function respondToExtensionDialog(
  dialog: ExtensionDialog,
  response: Record<string, unknown>,
) {
  discardExtensionDialog(dialog.key);
  const controller = controllerByKey(dialog.controllerKey);
  if (
    !controller ||
    controller.disposed ||
    controller.runtimeId !== dialog.runtimeId ||
    controller.generation !== dialog.generation
  ) {
    return;
  }
  try {
    await rpc(controller, response);
    // An answered dialog is a common point for a workflow to open its next
    // session, and that swap is not reported by any event.
    watchSessionReplacement(controller);
  } catch (error) {
    setControllerError(controller, error);
  }
}

export function handleExtensionUIRequest(
  controller: SessionController,
  request: Record<string, unknown>,
) {
  const requestId = stringValue(request.id);
  const method = stringValue(request.method);
  if (!requestId || !method) return;

  if (isExtensionDialogMethod(method)) {
    const origin = extensionRequestOrigin(controller);
    const timeout =
      typeof request.timeout === "number" &&
      Number.isFinite(request.timeout) &&
      request.timeout > 0
        ? request.timeout
        : undefined;
    const dialog: ExtensionDialog = {
      key: extensionRequestKey(controller, requestId),
      requestId,
      method,
      title: stringValue(request.title) || extensionDialogTitle(method),
      controllerKey: controller.key,
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      projectName: origin.projectName,
      sessionName: origin.sessionName,
      ...(origin.workingDirectory
        ? { workingDirectory: origin.workingDirectory }
        : {}),
      draft: typeof request.prefill === "string" ? request.prefill : "",
      ...(typeof request.message === "string"
        ? { message: request.message }
        : {}),
      ...(Array.isArray(request.options)
        ? {
            options: request.options.filter(
              (option): option is string => typeof option === "string",
            ),
          }
        : {}),
      ...(typeof request.placeholder === "string"
        ? { placeholder: request.placeholder }
        : {}),
      ...(typeof request.prefill === "string"
        ? { prefill: request.prefill }
        : {}),
      ...(timeout ? { timeout } : {}),
    };
    if (state.extensionDialogs.some((item) => item.key === dialog.key)) return;
    state.extensionDialogs.push(dialog);
    if (!isControllerSelected(controller)) controller.unread = true;
    if (timeout) {
      extensionDialogTimeouts.set(
        dialog.key,
        setTimeout(() => discardExtensionDialog(dialog.key), timeout),
      );
    }
    return;
  }

  if (method === "notify" && typeof request.message === "string") {
    const origin = extensionRequestOrigin(controller);
    const type = extensionNotificationType(request.notifyType);
    const notification: ExtensionNotification = {
      key: extensionRequestKey(controller, requestId),
      message: request.message,
      type,
      projectName: origin.projectName,
      sessionName: origin.sessionName,
      ...(origin.workingDirectory
        ? { workingDirectory: origin.workingDirectory }
        : {}),
    };
    dismissExtensionNotification(notification.key);
    state.extensionNotifications.push(notification);
    while (state.extensionNotifications.length > 4) {
      const oldest = state.extensionNotifications[0];
      if (oldest) dismissExtensionNotification(oldest.key);
    }
    extensionNotificationTimeouts.set(
      notification.key,
      setTimeout(() => dismissExtensionNotification(notification.key), 8_000),
    );
    return;
  }

  if (method === "setStatus") return;

  if (method === "set_editor_text" && typeof request.text === "string") {
    controller.draft = request.text;
    const session = ephemeralSessionByController(controller.key);
    if (session?.phantom) session.title = draftTitle(request.text);
  }
}

export function isExtensionDialogMethod(
  method: string,
): method is ExtensionDialogMethod {
  return (
    method === "select" ||
    method === "confirm" ||
    method === "input" ||
    method === "editor"
  );
}

export function extensionDialogTitle(method: ExtensionDialogMethod): string {
  if (method === "select") return "Choose an option";
  if (method === "confirm") return "Confirm";
  if (method === "input") return "Enter a value";
  return "Edit text";
}

export function extensionNotificationType(
  value: unknown,
): ExtensionNotificationType {
  return value === "warning" || value === "error" ? value : "info";
}

export function extensionRequestOrigin(controller: SessionController) {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  const ephemeral = ephemeralSessionByController(controller.key);
  const saved = project?.sessions.find(
    (session) => session.id === controller.sessionId,
  );
  return {
    projectName: project?.name || controller.projectPath,
    sessionName:
      controller.sessionName ||
      ephemeral?.title ||
      saved?.title ||
      firstUserMessage(controller) ||
      "New session",
    // A remote project's files are on the other host, where nothing local can
    // open them, so its paths are left as text.
    ...(project && !project.connectionString
      ? { workingDirectory: project.workingDirectory }
      : {}),
  };
}

export function extensionRequestKey(
  controller: SessionController,
  requestId: string,
): string {
  return `${controller.runtimeId}:${controller.generation}:${requestId}`;
}

export function discardExtensionDialog(key: string) {
  const timeout = extensionDialogTimeouts.get(key);
  if (timeout) clearTimeout(timeout);
  extensionDialogTimeouts.delete(key);
  const index = state.extensionDialogs.findIndex(
    (dialog) => dialog.key === key,
  );
  if (index >= 0) state.extensionDialogs.splice(index, 1);
}

export function discardControllerDialogs(
  controller: SessionController,
  generation?: number,
) {
  for (const dialog of [...state.extensionDialogs]) {
    if (
      dialog.controllerKey === controller.key &&
      (generation === undefined || dialog.generation === generation)
    ) {
      discardExtensionDialog(dialog.key);
    }
  }
}

export function dismissExtensionNotification(key: string) {
  const timeout = extensionNotificationTimeouts.get(key);
  if (timeout) clearTimeout(timeout);
  extensionNotificationTimeouts.delete(key);
  const index = state.extensionNotifications.findIndex(
    (notification) => notification.key === key,
  );
  if (index >= 0) state.extensionNotifications.splice(index, 1);
}

export function clearExtensionUiState() {
  for (const timeout of extensionDialogTimeouts.values()) clearTimeout(timeout);
  extensionDialogTimeouts.clear();
  state.extensionDialogs.splice(0);
  for (const timeout of extensionNotificationTimeouts.values()) {
    clearTimeout(timeout);
  }
  extensionNotificationTimeouts.clear();
  state.extensionNotifications.splice(0);
}

export async function handleBridgeEvent(event: PiBridgeEvent) {
  const controller = controllerByRuntimeId(event.runtimeId);
  if (!controller) return;
  touchController(controller);
  if (event.kind === "started") {
    if (event.generation > controller.generation) {
      discardControllerDialogs(controller);
      controller.generation = event.generation;
    }
    return;
  }
  if (event.generation !== controller.generation) return;

  if (event.kind === "rpc" && event.line) {
    let value: unknown;
    try {
      value = JSON.parse(event.line);
    } catch {
      return;
    }
    await handleRpc(controller, value);
    return;
  }
  if (event.kind === "stderr") return;
  if (event.kind === "error") {
    const message = event.message || "The Pi connection failed.";
    if (controller.connectingRemote) {
      presentRemoteConnectionError(controller, message);
      return;
    }
    if (controller.pendingPrompt) cancelPendingPrompt(controller, message);
    else setControllerError(controller, message);
    return;
  }
  if (event.kind === "exited") {
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    discardControllerDialogs(controller, event.generation);
    const message =
      event.code === 0
        ? ""
        : event.message || "The Pi process stopped unexpectedly.";
    if (controller.connectingRemote) {
      presentRemoteConnectionError(
        controller,
        message || "The remote Pi process stopped before it was ready.",
      );
      return;
    }
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message || "The Pi process stopped.");
    }
    controller.ready = false;
    controller.streaming = false;
    controller.stopping = false;
    controller.starting = false;
    controller.working = false;
    controller.runStateRequestId = "";
    controller.generation = 0;
    controller.status = message;
  }
}

export async function handleRpc(controller: SessionController, value: unknown) {
  const event = asRecord(value);
  if (!event) return;
  const type = stringValue(event.type);
  if (type === "extension_ui_request") {
    handleExtensionUIRequest(controller, event);
    return;
  }
  if (type === "response") {
    await handleResponse(controller, event);
    return;
  }
  if (type === "agent_start") {
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    controller.localErrors = [];
    controller.streaming = true;
    controller.stopping = false;
    controller.working = true;
    controller.status = "";
    const stateRequestId = nextRequestId("run-state");
    controller.runStateRequestId = stateRequestId;
    await rpc(controller, { id: stateRequestId, type: "get_state" });
    return;
  }
  if (type === "message_update") {
    const delta = asRecord(event.assistantMessageEvent);
    const deltaType = stringValue(delta?.type);
    if (deltaType === "text_delta") {
      appendStream(controller, "assistant", stringValue(delta?.delta));
      if (!isControllerSelected(controller)) controller.unread = true;
    }
    if (deltaType === "thinking_delta") {
      appendStream(controller, "thinking", stringValue(delta?.delta));
    }
    return;
  }
  // A turn that failed is settled from Pi's messages soon after, but only once
  // the run ends, and a retry can hold that off for the length of its backoff.
  if (type === "message_end") {
    const failure = messageFailure(event.message);
    if (failure) pushError(controller, failure);
    return;
  }
  /**
   * A failed compaction is reported once and kept nowhere: Pi's message list
   * has no entry for it, so the row is held on the controller and re-appended
   * to every list hydrated until the next run begins.
   */
  if (type === "compaction_end") {
    const failure = stringValue(event.errorMessage);
    if (failure) {
      controller.localErrors.push(failure);
      pushError(controller, failure);
    }
    return;
  }
  if (type === "tool_execution_start") {
    const toolCallId = stringValue(event.toolCallId);
    const toolName = stringValue(event.toolName) || "tool";
    controller.messages.push({
      id: `stream-tool-${controller.streamSequence++}`,
      kind: "tool",
      text: toolSummary(event.args),
      toolCallId,
      toolName,
      toolRunning: true,
      toolErrored: false,
      toolArguments: toolArgumentsText(event.args),
    });
    return;
  }
  if (type === "tool_execution_end") {
    const toolCallId = stringValue(event.toolCallId);
    const tool = [...controller.messages]
      .reverse()
      .find(
        (entry) => entry.kind === "tool" && entry.toolCallId === toolCallId,
      );
    if (tool) {
      tool.toolRunning = false;
      tool.toolErrored = event.isError === true;
      tool.toolResult = toolResultText(asRecord(event.result)?.content);
    }
    return;
  }
  if (type === "agent_settled") {
    clearAbortWatch(controller);
    controller.syncing = true;
    controller.working = false;
    controller.streaming = false;
    controller.stopping = false;
    controller.status = "";
    if (!isControllerSelected(controller)) controller.unread = true;
    watchSessionReplacement(controller);
    await rpc(controller, {
      id: nextRequestId("settled-state"),
      type: "get_state",
    });
    return;
  }
  // Pi announces every rename, whether it came from Tau's header, one of Pi's
  // own commands, or an extension, so the name is only ever read back from Pi.
  if (type === "session_info_changed") {
    applySessionName(controller, stringValue(event.name));
    await persistSessionName(controller);
    return;
  }
  if (type === "auto_retry_start") {
    controller.status = retryStatus(event);
    return;
  }
  if (type === "extension_error") {
    controller.status = stringValue(event.error) || "A Pi extension failed.";
  }
}

export async function handleResponse(
  controller: SessionController,
  response: Record<string, unknown>,
) {
  const command = stringValue(response.command);
  const responseId = stringValue(response.id);
  const resolvesRunState =
    command === "get_state" &&
    Boolean(controller.runStateRequestId) &&
    responseId === controller.runStateRequestId;
  if (resolvesRunState) controller.runStateRequestId = "";
  const resolvesReplacementProbe =
    command === "get_state" &&
    Boolean(controller.replacementProbeRequestId) &&
    responseId === controller.replacementProbeRequestId;
  if (resolvesReplacementProbe) controller.replacementProbeRequestId = "";
  const resolvesCommandSync =
    command === "get_state" &&
    Boolean(controller.commandSyncRequestId) &&
    responseId === controller.commandSyncRequestId;
  if (resolvesCommandSync) controller.commandSyncRequestId = "";
  const resolvesAbortProbe =
    command === "get_state" &&
    Boolean(controller.abortProbeRequestId) &&
    responseId === controller.abortProbeRequestId;
  if (resolvesAbortProbe) controller.abortProbeRequestId = "";
  if (
    command === "prompt" &&
    Boolean(controller.commandPromptRequestId) &&
    responseId === controller.commandPromptRequestId
  ) {
    controller.commandPromptRequestId = "";
    if (response.success === true) {
      controller.working = controller.streaming;
      await syncAfterCommand(controller);
      return;
    }
  }
  if (response.success !== true) {
    const error = asRecord(response.error);
    controller.status =
      stringValue(error?.message) ||
      stringValue(response.error) ||
      `Pi rejected ${command || "the request"}.`;
    const pending = controller.pendingPrompt;
    const failedPendingSetting =
      Boolean(pending?.settingsRequestId) &&
      responseId === pending?.settingsRequestId;
    if (failedPendingSetting) {
      cancelPendingPrompt(controller, controller.status);
      return;
    }
    const failedPendingRequest =
      Boolean(pending) &&
      ((command === "get_state" && responseId === pending?.stateRequestId) ||
        (command === "get_messages" &&
          responseId === pending?.messagesRequestId));
    if (failedPendingRequest) {
      cancelPendingPrompt(controller, controller.status);
    }
    if (command === "get_state") {
      controller.syncing = false;
      if (responseId === controller.bootstrapStateRequestId) {
        controller.bootstrapStateRequestId = "";
        controller.starting = false;
      }
      releaseRuntime(controller);
    }
    if (
      command === "get_messages" &&
      responseId === controller.startMessagesRequestId
    ) {
      controller.startMessagesRequestId = "";
      controller.starting = false;
      controller.syncing = false;
    }
    if (command === "prompt") controller.working = false;
    if (command === "abort") controller.stopping = false;
    // A rejected rename leaves the optimistic name on screen, so Pi's name is
    // read back instead of being guessed.
    if (command === "set_session_name") {
      await rpc(controller, {
        id: nextRequestId("session-name-state"),
        type: "get_state",
      });
    }
    return;
  }
  const data = asRecord(response.data);

  if (command === "get_state" && data) {
    const model = asRecord(data.model);
    controller.currentModelProvider = stringValue(model?.provider);
    controller.currentModelId = stringValue(model?.id);
    controller.currentModelName = stringValue(model?.name);
    controller.currentEffort = normalizeEffort(data.thinkingLevel);
    const piSessionId = stringValue(data.sessionId);
    const piSessionPath = stringValue(data.sessionFile);
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.stateRequestId) &&
      responseId === pending?.stateRequestId;
    const resolvesBootstrap =
      Boolean(controller.bootstrapStateRequestId) &&
      responseId === controller.bootstrapStateRequestId;
    // Pi opens a session it cannot find as a fresh one under the very path it
    // was handed, so a bootstrap that answers with the requested path under
    // another id is Pi reporting that the session was never saved. Every other
    // session Pi reports here is one it holds, replacement included, since a
    // replacement always brings its own path.
    const unsavedSession =
      resolvesBootstrap &&
      !controller.phantom &&
      Boolean(controller.bootstrapSessionPath) &&
      piSessionPath === controller.bootstrapSessionPath &&
      Boolean(piSessionId) &&
      piSessionId !== controller.sessionId;
    const sessionChanged =
      !controller.phantom &&
      !unsavedSession &&
      Boolean(piSessionId && piSessionPath) &&
      (controller.sessionId !== piSessionId ||
        controller.sessionPath !== piSessionPath);
    controller.sessionName = stringValue(data.sessionName);
    controller.ready = true;
    controller.streaming = data.isStreaming === true;
    controller.stopping = false;
    controller.status =
      resolvesAbortProbe && controller.streaming
        ? unstoppedStatus(controller)
        : "";
    finishRemoteConnection(controller);

    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom && !unsavedSession) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }

    if (sessionChanged) {
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      controller.messages = [];
      controller.models = [];
      controller.efforts = [];
      controller.commands = [];
      controller.commandsLoaded = false;
      controller.pendingEffort = "";
      controller.syncing = true;
    }
    controller.working = controller.streaming || Boolean(pending);

    if (unsavedSession) await retireUnsavedSession(controller);
    if (controller.disposed) return;

    if (
      controller.sessionId &&
      controller.sessionPath &&
      (!resolvesRunState || sessionChanged) &&
      // A command's session is not final until Pi finishes handling it, so
      // registering now would record a session the command is about to
      // replace, and Pi never writes one that holds no assistant message.
      !(resolvesPending && pending?.command)
    ) {
      await registerConnectedSession(controller);
    }
    if (controller.disposed) return;

    if (resolvesPending && pending) {
      controller.bootstrapStateRequestId = "";
      await applyPendingSessionSettings(controller);
      return;
    }
    if (resolvesRunState && !sessionChanged) return;
    if (resolvesReplacementProbe && !sessionChanged) {
      releaseIdleRuntimes();
      return;
    }
    if (resolvesCommandSync && controller.streaming && !sessionChanged) return;
    // A run that outlived its abort is still writing the transcript, so the
    // probe only reports on it. A run that did stop falls through and syncs.
    if (resolvesAbortProbe && controller.streaming && !sessionChanged) return;

    if (sessionChanged) {
      await rpc(controller, {
        id: nextRequestId("replacement-models"),
        type: "get_available_models",
      });
      await rpc(controller, {
        id: nextRequestId("replacement-commands"),
        type: "get_commands",
      });
    }

    const messagesRequestId = nextRequestId("messages");
    if (resolvesBootstrap) {
      controller.startMessagesRequestId = messagesRequestId;
      controller.bootstrapStateRequestId = "";
    }
    await rpc(controller, {
      id: nextRequestId("efforts"),
      type: "get_available_thinking_levels",
    });
    await rpc(controller, { id: messagesRequestId, type: "get_messages" });
    return;
  }

  if (command === "get_messages" && data) {
    controller.syncing = false;
    controller.messages = appendLocalErrors(
      hydrateTranscript(
        Array.isArray(data.messages) ? data.messages : [],
        controller.messages,
      ),
      controller.localErrors,
    );
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.messagesRequestId) &&
      responseId === pending?.messagesRequestId;
    if (resolvesPending) {
      await dispatchPendingPrompt(controller);
    } else if (responseId === controller.startMessagesRequestId) {
      controller.startMessagesRequestId = "";
      controller.starting = false;
    }
    // A hidden session that finishes hydrating has nothing left to wait for,
    // so this is where a runtime the user has moved on from is accounted for.
    releaseIdleRuntimes();
    return;
  }

  if (command === "get_available_models" && data) {
    controller.models = (Array.isArray(data.models) ? data.models : [])
      .map((value): ModelOption | undefined => {
        const model = asRecord(value);
        const provider = stringValue(model?.provider);
        const id = stringValue(model?.id);
        if (!provider || !id) return undefined;
        return {
          provider,
          id,
          name: stringValue(model?.name) || id,
          reasoning: model?.reasoning === true,
        };
      })
      .filter((model): model is ModelOption => Boolean(model));
    return;
  }

  if (command === "get_available_thinking_levels" && data) {
    controller.efforts = (Array.isArray(data.levels) ? data.levels : [])
      .map(normalizeEffort)
      .filter((level, index, levels) => levels.indexOf(level) === index);
    return;
  }

  if (command === "get_commands" && data) {
    controller.commands = (Array.isArray(data.commands) ? data.commands : [])
      .map(commandOption)
      .filter((command): command is CommandOption => Boolean(command));
    controller.commandsLoaded = true;
    return;
  }

  if (command === "prompt") {
    controller.working = controller.streaming;
    return;
  }

  if (command === "set_model") {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === "model" &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = "";
      pending.settingsStep = "";
      controller.currentModelProvider = pending.selectedModelProvider;
      controller.currentModelId = pending.selectedModelId;
      controller.currentModelName = pending.selectedModelName;
      await applyPendingSessionEffort(controller, true);
      return;
    }
    await rpc(controller, {
      id: nextRequestId("model-state"),
      type: "get_state",
    });
    return;
  }

  if (command === "set_thinking_level") {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === "effort" &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = "";
      pending.settingsStep = "";
      controller.currentEffort = pending.selectedEffort;
      controller.status = "";
      await requestPendingMessages(controller);
      return;
    }
    if (controller.pendingEffort) {
      controller.currentEffort = controller.pendingEffort;
    }
    controller.pendingEffort = "";
    controller.status = "";
  }
}

export async function applyPendingSessionSettings(
  controller: SessionController,
) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  const modelChanged =
    Boolean(pending.selectedModelProvider && pending.selectedModelId) &&
    (pending.selectedModelProvider !== controller.currentModelProvider ||
      pending.selectedModelId !== controller.currentModelId);
  if (!modelChanged) {
    await applyPendingSessionEffort(controller, false);
    return;
  }

  const requestId = nextRequestId("initial-model");
  pending.settingsRequestId = requestId;
  pending.settingsStep = "model";
  await rpc(controller, {
    id: requestId,
    type: "set_model",
    provider: pending.selectedModelProvider,
    modelId: pending.selectedModelId,
  });
}

export async function applyPendingSessionEffort(
  controller: SessionController,
  force: boolean,
) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  if (!force && pending.selectedEffort === controller.currentEffort) {
    await requestPendingMessages(controller);
    return;
  }

  const requestId = nextRequestId("initial-effort");
  pending.settingsRequestId = requestId;
  pending.settingsStep = "effort";
  await rpc(controller, {
    id: requestId,
    type: "set_thinking_level",
    level: pending.selectedEffort,
  });
}

export async function requestPendingMessages(controller: SessionController) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  const messagesRequestId = nextRequestId("messages");
  pending.messagesRequestId = messagesRequestId;
  await rpc(controller, {
    id: nextRequestId("efforts"),
    type: "get_available_thinking_levels",
  });
  await rpc(controller, { id: messagesRequestId, type: "get_messages" });
}

export async function dispatchPendingPrompt(controller: SessionController) {
  const prompt = controller.pendingPrompt;
  if (!prompt) return;
  controller.pendingPrompt = undefined;
  controller.starting = false;
  if (
    !prompt.command &&
    !controller.messages.some((message) => message.id === prompt.optimisticId)
  ) {
    controller.messages.push({
      id: prompt.optimisticId,
      kind: "user",
      text: prompt.message,
    });
  }
  controller.working = true;
  try {
    const requestId = nextRequestId("prompt");
    if (prompt.command) controller.commandPromptRequestId = requestId;
    await rpc(controller, {
      id: requestId,
      type: "prompt",
      message: prompt.message,
    });
  } catch (error) {
    controller.working = false;
    controller.commandPromptRequestId = "";
    controller.draft = prompt.message;
    setControllerError(controller, error);
  }
}

export async function syncAfterCommand(controller: SessionController) {
  if (controller.disposed || !controller.generation) return;
  const requestId = nextRequestId("command-sync");
  controller.commandSyncRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch (error) {
    controller.commandSyncRequestId = "";
    setControllerError(controller, error);
  }
}

export function watchSessionReplacement(controller: SessionController) {
  clearSessionReplacementWatch(controller);
  if (controller.disposed || !controller.generation) return;
  replacementProbeTimers.set(
    controller.key,
    replacementProbeDelays.map((delay, index) =>
      setTimeout(() => {
        void probeSessionReplacement(
          controller,
          index === replacementProbeDelays.length - 1,
        );
      }, delay),
    ),
  );
}

export async function probeSessionReplacement(
  controller: SessionController,
  last: boolean,
) {
  if (last) replacementProbeTimers.delete(controller.key);
  if (
    controller.disposed ||
    !controller.generation ||
    controller.starting ||
    controller.streaming ||
    controller.working
  ) {
    return;
  }
  const requestId = nextRequestId("replacement-probe");
  controller.replacementProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch {
    controller.replacementProbeRequestId = "";
  }
}

export function clearSessionReplacementWatch(controller: SessionController) {
  const timers = replacementProbeTimers.get(controller.key);
  if (!timers) return;
  replacementProbeTimers.delete(controller.key);
  for (const timer of timers) clearTimeout(timer);
}

export function watchingSessionReplacement(
  controller: SessionController,
): boolean {
  return replacementProbeTimers.has(controller.key);
}

export function watchAbort(controller: SessionController) {
  clearAbortWatch(controller);
  if (controller.disposed || !controller.generation) return;
  abortProbeTimers.set(
    controller.key,
    setTimeout(() => {
      void probeAbort(controller);
    }, abortAcknowledgeDelay),
  );
}

export async function probeAbort(controller: SessionController) {
  abortProbeTimers.delete(controller.key);
  if (controller.disposed || !controller.generation || !controller.stopping) {
    return;
  }
  const requestId = nextRequestId("abort-probe");
  controller.abortProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch (error) {
    controller.abortProbeRequestId = "";
    controller.stopping = false;
    setControllerError(controller, error);
  }
}

export function clearAbortWatch(controller: SessionController) {
  const timer = abortProbeTimers.get(controller.key);
  if (!timer) return;
  abortProbeTimers.delete(controller.key);
  clearTimeout(timer);
}

export function unstoppedStatus(controller: SessionController): string {
  const tool = [...controller.messages]
    .reverse()
    .find((entry) => entry.kind === "tool" && entry.toolRunning);
  return tool?.toolName
    ? `Pi is still running ${tool.toolName} and stops once it returns.`
    : "Pi has not stopped yet and stops once its current work finishes.";
}

export function invokesExtensionCommand(
  controller: SessionController,
  message: string,
): boolean {
  if (!message.startsWith("/")) return false;
  // Pi takes the command name from the first space, so anything else is a
  // prompt for the model even when it opens with a slash.
  const space = message.indexOf(" ");
  const name = space < 0 ? message.slice(1) : message.slice(1, space);
  return (
    Boolean(name) &&
    controller.commands.some(
      (command) => command.name === name && command.source === "extension",
    )
  );
}

export function applySessionName(controller: SessionController, name: string) {
  controller.sessionName = name;
  const session = ephemeralSessionByController(controller.key);
  if (name && session && !session.phantom) session.title = name;
}

export async function persistSessionName(controller: SessionController) {
  if (controller.phantom || !controller.sessionId || !controller.sessionPath) {
    return;
  }
  await registerConnectedSession(controller);
}

export async function registerConnectedSession(controller: SessionController) {
  try {
    const workspace = await invoke<WorkspaceSnapshot>("register_session", {
      projectPath: controller.projectPath,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
      sessionName:
        controller.sessionName || firstUserMessage(controller) || null,
      lastUserMessageAt:
        controller.lastUserMessageAt > 0 ? controller.lastUserMessageAt : null,
    });
    if (controller.disposed) return;
    state.workspace = workspace;
    if (workspaceContainsSession(controller)) {
      removeRegisteredEphemeralSession(controller);
    }
    if (isControllerSelected(controller)) {
      state.activeSessionId = controller.sessionId;
      state.activeSessionPath = controller.sessionPath;
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_session", {
        projectPath: controller.projectPath,
        sessionId: controller.sessionId,
      });
    }
  } catch (error) {
    setControllerError(controller, error);
  }
}

export async function persistExpandedProject(projectPath: string) {
  try {
    state.workspace = await invoke<WorkspaceSnapshot>("set_project_collapsed", {
      path: projectPath,
      collapsed: false,
    });
  } catch (error) {
    setActiveError(error);
  }
}

export async function persistProjectSelection(
  projectPath: string,
  controller: SessionController,
) {
  try {
    if (controller.phantom) {
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_project", {
        path: projectPath,
      });
    } else {
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_session", {
        projectPath,
        sessionId: controller.sessionId,
      });
    }
  } catch (error) {
    setControllerError(controller, error);
  }
}

export function materializePendingSession(
  controller: SessionController,
  sessionId: string,
  sessionPath: string,
) {
  const session = ephemeralSessionByController(controller.key);
  controller.sessionId = sessionId;
  controller.sessionPath = sessionPath;
  controller.phantom = false;
  if (session) {
    session.id = sessionId;
    session.path = sessionPath;
    session.phantom = false;
    session.title = draftTitle(controller.pendingPrompt?.message ?? "");
  }
  if (isControllerSelected(controller)) {
    state.activeSessionId = sessionId;
    state.activeSessionPath = sessionPath;
  }
}

export function cancelPendingPrompt(
  controller: SessionController,
  error: unknown,
) {
  const prompt = controller.pendingPrompt;
  if (!prompt) {
    setControllerError(controller, error);
    return;
  }
  controller.pendingPrompt = undefined;
  controller.starting = false;
  controller.working = false;
  controller.messages = controller.messages.filter(
    (message) => message.id !== prompt.optimisticId,
  );
  controller.draft = prompt.message;
  setControllerError(controller, error);
}

export function pushError(controller: SessionController, text: string) {
  controller.messages.push({
    id: `stream-error-${controller.streamSequence++}`,
    kind: "error",
    text,
  });
  if (!isControllerSelected(controller)) controller.unread = true;
}

/** How much of a reason a retry line carries before the composer is pushed down. */
export const retryReasonLimit = 120;

export function retryStatus(event: Record<string, unknown>): string {
  const attempt = String(event.attempt ?? "");
  const maxAttempts = event.maxAttempts ? `/${String(event.maxAttempts)}` : "";
  const reason = describePiError(stringValue(event.errorMessage)).message;
  const trimmed =
    reason.length > retryReasonLimit
      ? `${reason.slice(0, retryReasonLimit).trimEnd()}…`
      : reason;
  return trimmed
    ? `Retrying (${attempt}${maxAttempts}): ${trimmed}`
    : `Retrying (${attempt}${maxAttempts})…`;
}

export function appendStream(
  controller: SessionController,
  kind: "assistant" | "thinking",
  delta: string,
) {
  if (!delta) return;
  const last = controller.messages[controller.messages.length - 1];
  if (last?.kind === kind && last.id.startsWith("stream-")) {
    last.text += delta;
    return;
  }
  controller.messages.push({
    id: `stream-${kind}-${controller.streamSequence++}`,
    kind,
    text: delta,
  });
}

export async function retireUnsavedSession(controller: SessionController) {
  const staleSessionId = controller.sessionId;
  if (workspaceContainsSession(controller)) {
    try {
      state.workspace = await invoke<WorkspaceSnapshot>("archive_session", {
        projectPath: controller.projectPath,
        sessionId: staleSessionId,
      });
    } catch {
      // A row Tau cannot retire is still worth replacing with a session that
      // works, and the message below says why it is there.
    }
  }
  if (controller.disposed) return;

  const previous = ephemeralSessionByController(controller.key);
  if (previous) removeEphemeralSession(previous, false);
  const session = createPhantomSession(controller.projectPath, controller.key);
  state.ephemeralSessions.push(session);

  controller.phantom = true;
  controller.sessionId = session.id;
  controller.sessionPath = "";
  controller.sessionName = "";
  controller.lastUserMessageAt = 0;
  controller.messages = [];
  if (isControllerSelected(controller)) {
    state.activeSessionId = session.id;
    state.activeSessionPath = "";
  }
  controller.status =
    "That session was never saved by Pi, so this is a new one.";
}

export function removeEmptyActivePhantom() {
  const controller = activeController.value;
  const session = controller
    ? ephemeralSessionByController(controller.key)
    : undefined;
  if (
    !controller ||
    !session?.phantom ||
    controller.draft.trim() ||
    controller.working ||
    controllerHasPendingDialog(controller) ||
    controller.messages.length > 0
  ) {
    return;
  }
  removeEphemeralSession(session, true);
}

export function removeRegisteredEphemeralSession(
  controller: SessionController,
) {
  const session = ephemeralSessionByController(controller.key);
  if (session && !session.phantom) removeEphemeralSession(session, false);
}

export function removeEphemeralSession(
  session: EphemeralSession,
  removeController: boolean,
) {
  const index = state.ephemeralSessions.indexOf(session);
  if (index >= 0) state.ephemeralSessions.splice(index, 1);
  if (removeController) {
    const controller = controllerByKey(session.controllerKey);
    if (controller) {
      controller.disposed = true;
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      void stopControllerProcess(controller);
      const controllerIndex = state.controllers.indexOf(controller);
      if (controllerIndex >= 0) state.controllers.splice(controllerIndex, 1);
      if (state.activeControllerKey === controller.key) clearActiveSession();
    }
  }
}

export function removeProjectUiState(projectPath: string) {
  state.ephemeralSessions = state.ephemeralSessions.filter(
    (session) => session.projectPath !== projectPath,
  );
  for (const controller of state.controllers) {
    if (controller.projectPath !== projectPath) continue;
    controller.disposed = true;
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
  }
  state.controllers = state.controllers.filter(
    (controller) => controller.projectPath !== projectPath,
  );
}

export function canReleaseRuntime(controller: SessionController): boolean {
  return (
    !isControllerSelected(controller) &&
    controller.ready &&
    !controller.streaming &&
    !controller.working &&
    !controller.starting &&
    !controller.syncing &&
    !controllerHasPendingDialog(controller) &&
    !watchingSessionReplacement(controller)
  );
}

export function releaseRuntime(controller: SessionController | undefined) {
  if (!controller || !canReleaseRuntime(controller)) return;
  void stopControllerProcess(controller);
}

export function releaseIdleRuntimes() {
  const idle = state.controllers
    .filter((controller) => canReleaseRuntime(controller))
    .sort(
      (first, second) => second.lastActiveSequence - first.lastActiveSequence,
    );
  for (const controller of idle.slice(idleRuntimeLimit)) {
    void stopControllerProcess(controller);
  }
}

export async function stopControllerProcess(controller: SessionController) {
  if (!controller.generation && !controller.starting) return;
  const generation = controller.generation;
  clearSessionReplacementWatch(controller);
  clearAbortWatch(controller);
  discardControllerDialogs(controller, generation);
  try {
    await invoke("stop_pi", { runtimeId: controller.runtimeId });
  } catch (error) {
    setControllerError(controller, error);
  }
  if (controller.generation !== generation) return;
  controller.generation = 0;
  controller.ready = false;
  controller.streaming = false;
  controller.stopping = false;
  controller.starting = false;
  controller.working = false;
  controller.connectingRemote = false;
  controller.syncing = false;
  controller.runStateRequestId = "";
  controller.abortProbeRequestId = "";

  if (
    isControllerSelected(controller) &&
    !controller.disposed &&
    !controller.phantom
  ) {
    const project = state.workspace?.projects.find(
      (item) => item.path === controller.projectPath,
    );
    if (project) {
      void startController(controller, project, controller.sessionPath);
    }
  }
}
