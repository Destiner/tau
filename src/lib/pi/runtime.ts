/*
 * Pi protocol layer: the bridge events and RPC dispatch that drive a session
 * runtime, the extension UI requests they surface, and the controller
 * lifecycle (start, release, dispose). Mirrors src-tauri/src/pi.rs.
 */
import { invoke } from '@tauri-apps/api/core';

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
  setControllerLifecycle,
  state,
  touchController,
  workspaceContainsSession,
  type EphemeralSession,
  type ExtensionDialog,
  type ExtensionDialogMethod,
  type ProjectSummary,
  type SessionController,
  type WorkspaceSnapshot,
} from '../../composables/state';
import type { CommandOption } from '../commands';
import {
  invokeTraced,
  recordRpcResponseAnomaly,
  recordStreamAggregate,
  startRpcSpan,
} from '../telemetry';
import type { PiRpcMethod, PiRpcOutcome } from '../telemetry/attributes';
import type { TraceContext } from '../telemetry/trace-context';

import type { PiBridgeEvent } from './bridge';
import { describePiError } from './error';
import type { ModelOption } from './model-scope';
import {
  asRecord,
  contentText,
  hydrateTranscript,
  localErrorId,
  mergeLocalEntries,
  messageFailure,
  parseSkillBlock,
  stringValue,
  toolArgumentsText,
  toolResultText,
  toolSummary,
  type TranscriptNoticeType,
} from './transcript';

async function sendPhantomMessage(
  controller: SessionController,
  message: string,
  command: boolean,
  parentContext?: TraceContext,
): Promise<void> {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project || !controller.phantom) return;

  const optimisticId = `optimistic-user-${Date.now()}`;
  controller.pendingPrompt = {
    message,
    optimisticId,
    command,
    stateRequestId: '',
    messagesRequestId: '',
    selectedModelProvider: controller.currentModelProvider,
    selectedModelId: controller.currentModelId,
    selectedModelName: controller.currentModelName,
    selectedEffort: controller.currentEffort,
    settingsRequestId: '',
    settingsStep: '',
    telemetryContext: parentContext,
  };
  controller.promptSubmitting = true;
  setControllerLifecycle(
    controller,
    { working: true },
    'phantom_prompt_start',
    parentContext,
  );
  if (!command) appendOptimisticPrompt(controller, message, optimisticId);
  controller.status = '';

  if (controller.ready && controller.generation) {
    setControllerLifecycle(
      controller,
      { starting: true },
      'phantom_prompt_resume',
      parentContext,
    );
    const stateRequestId = nextRequestId('state');
    controller.pendingPrompt.stateRequestId = stateRequestId;
    try {
      await rpc(
        controller,
        { id: stateRequestId, type: 'get_state' },
        parentContext,
      );
    } catch (error) {
      cancelPendingPrompt(controller, error);
    }
    return;
  }
  await startController(controller, project, undefined, true, parentContext);
}

async function startController(
  controller: SessionController,
  project: ProjectSummary,
  sessionPath?: string,
  preserveMessages = false,
  parentContext?: TraceContext,
): Promise<void> {
  if (controller.starting || controller.disposed) return;
  setControllerLifecycle(
    controller,
    {
      ready: false,
      starting: true,
      stopping: false,
      connectingRemote: Boolean(project.connectionString),
    },
    'controller_start',
    parentContext,
  );
  controller.bootstrapStateRequestId = '';
  controller.bootstrapSessionPath = sessionPath ?? '';
  controller.runStateRequestId = '';
  controller.startMessagesRequestId = '';
  controller.commandPromptRequestId = '';
  controller.commandSyncRequestId = '';
  controller.replacementProbeRequestId = '';
  controller.pendingSessionRename = undefined;
  controller.abortProbeRequestId = '';
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
  controller.status = '';
  discardControllerDialogs(controller);
  void refreshModelScope(controller, project, parentContext);

  try {
    if (project.connectionString) {
      controller.generation = await invokeTraced<number>(
        'start_pi_remote',
        {
          runtimeId: controller.runtimeId,
          connectionString: project.connectionString,
          workingDirectory: project.workingDirectory,
          sessionPath: sessionPath ?? null,
        },
        parentContext,
      );
    } else {
      controller.generation = await invokeTraced<number>(
        'start_pi',
        {
          runtimeId: controller.runtimeId,
          projectPath: project.workingDirectory,
          sessionPath: sessionPath ?? null,
        },
        parentContext,
      );
    }
    if (controller.disposed) {
      await invokeTraced(
        'stop_pi',
        { runtimeId: controller.runtimeId },
        parentContext,
      );
      return;
    }
    await requestBootstrap(controller, parentContext);
  } catch (error) {
    setControllerLifecycle(
      controller,
      { starting: false, connectingRemote: false, syncing: false },
      'controller_start_failed',
      parentContext,
    );
    if (project.connectionString)
      presentRemoteConnectionError(controller, error);
    else setControllerError(controller, error);
    if (controller.pendingPrompt) cancelPendingPrompt(controller, error);
  }
}

async function refreshModelScope(
  controller: SessionController,
  project: ProjectSummary,
  parentContext?: TraceContext,
): Promise<void> {
  try {
    const patterns = project.connectionString
      ? await invokeTraced<string[]>(
          'read_remote_model_scope',
          { connectionString: project.connectionString },
          parentContext,
        )
      : await invokeTraced<string[]>('read_model_scope', {}, parentContext);
    if (controller.disposed || !Array.isArray(patterns)) return;
    controller.modelScope = patterns.filter(
      (pattern) => typeof pattern === 'string',
    );
  } catch {
    controller.modelScope = [];
  }
}

async function requestBootstrap(
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<void> {
  await rpc(
    controller,
    { id: nextRequestId('models'), type: 'get_available_models' },
    parentContext,
  );
  await rpc(
    controller,
    { id: nextRequestId('commands'), type: 'get_commands' },
    parentContext,
  );
  const stateRequestId = nextRequestId('state');
  controller.bootstrapStateRequestId = stateRequestId;
  if (controller.pendingPrompt) {
    controller.pendingPrompt.stateRequestId = stateRequestId;
  }
  await rpc(
    controller,
    { id: stateRequestId, type: 'get_state' },
    parentContext,
  );
}

/**
 * Bounds how long a Pi RPC span can stay pending before it is closed out as
 * timed out. Pi RPCs can legitimately run for a long time (a prompt with
 * tool use), so this is generous — it exists only so an RPC that never gets
 * a response (and is never otherwise abandoned) does not pin a span open
 * forever, not to police ordinary latency.
 */
const RPC_SPAN_TIMEOUT_MS = 10 * 60 * 1000;

interface PendingRpcSpan {
  end: (outcome: PiRpcOutcome) => void;
  context?: TraceContext;
  timeoutHandle: ReturnType<typeof setTimeout>;
  /** When this request was registered, in epoch milliseconds. Used only to
   * derive `oldestPendingRpcAgeMs` for the periodic state summary — never
   * sent as its own record, never per-request. */
  startedAt: number;
}

/**
 * Pi RPC spans in flight, keyed by runtime + generation + request id — the
 * exact key the plan calls for, and the same shape `send_pi`'s responses
 * carry. This is explicit request bookkeeping, not ambient "current span"
 * state: entries are looked up and removed by their own key, so concurrent
 * runtimes and generations cannot collide or leak into each other.
 */
const pendingRpcSpans = new Map<string, PendingRpcSpan>();

function rpcSpanKey(
  runtimeId: string,
  generation: number,
  requestId: string,
): string {
  return `${runtimeId}:${generation}:${requestId}`;
}

function registerPendingRpcSpan(
  key: string,
  end: (outcome: PiRpcOutcome) => void,
  context?: TraceContext,
): void {
  const previous = pendingRpcSpans.get(key);
  if (previous) {
    pendingRpcSpans.delete(key);
    clearTimeout(previous.timeoutHandle);
    previous.end('abandoned_duplicate_request');
  }
  const timeoutHandle = setTimeout(() => {
    pendingRpcSpans.delete(key);
    end('timeout');
  }, RPC_SPAN_TIMEOUT_MS);
  pendingRpcSpans.set(key, {
    end,
    context,
    timeoutHandle,
    startedAt: Date.now(),
  });
}

/** The number of Pi RPC spans currently pending a matching response,
 * timeout, or explicit abandonment. Read by `./telemetry/heartbeat` for the
 * periodic heartbeat/state-summary gauges; never exposes the map itself. */
function pendingRpcCount(): number {
  return pendingRpcSpans.size;
}

/** The age, in milliseconds, of the longest-pending RPC span, or `0` when
 * none are pending. One of the "ages of pending operations" the periodic
 * state summary records. */
function oldestPendingRpcAgeMs(): number {
  const now = Date.now();
  let oldest = 0;
  for (const pending of pendingRpcSpans.values()) {
    const age = now - pending.startedAt;
    if (age > oldest) oldest = age;
  }
  return oldest;
}

/** Ends the pending span for `key` with `outcome`, if one is still pending.
 * A key with nothing pending — an unmatched or duplicate response — is a
 * deliberate no-op: there is nothing to end, and the caller's own response
 * handling continues regardless. */
interface EndPendingRpcResult {
  matched: boolean;
  context?: TraceContext;
}

function endPendingRpcSpan(
  key: string,
  outcome: PiRpcOutcome,
): EndPendingRpcResult {
  const pending = pendingRpcSpans.get(key);
  if (!pending) return { matched: false };
  pendingRpcSpans.delete(key);
  clearTimeout(pending.timeoutHandle);
  pending.end(outcome);
  return { matched: true, context: pending.context };
}

/** Abandons every span pending for `runtimeId`/`generation` with `outcome`:
 * process exit, a generation change, a controller disposal or replacement,
 * or a stop path. Never touches spans for other runtimes or generations. */
function abandonPendingRpcSpans(
  runtimeId: string,
  generation: number,
  outcome: PiRpcOutcome,
): void {
  const prefix = `${runtimeId}:${generation}:`;
  for (const [key, pending] of pendingRpcSpans) {
    if (!key.startsWith(prefix)) continue;
    pendingRpcSpans.delete(key);
    clearTimeout(pending.timeoutHandle);
    pending.end(outcome);
  }
}

interface StreamAggregate {
  deltaCount: number;
  characterCount: number;
  startedAt: number;
  sessionId: string;
  controllerId: string;
}

/** Per-run streaming aggregates, keyed the same way as pending RPC spans.
 * Accumulates locally and is recorded as one bounded `pi.stream` span per
 * run — never one record per delta or token — when the run settles, is
 * abandoned, or the runtime stops. */
const streamAggregates = new Map<string, StreamAggregate>();

function streamAggregateKey(runtimeId: string, generation: number): string {
  return `${runtimeId}:${generation}`;
}

function resetStreamAggregate(runtimeId: string, generation: number): void {
  streamAggregates.delete(streamAggregateKey(runtimeId, generation));
}

function recordStreamDelta(controller: SessionController, delta: string): void {
  if (!delta) return;
  const key = streamAggregateKey(controller.runtimeId, controller.generation);
  const aggregate =
    streamAggregates.get(key) ??
    ({
      deltaCount: 0,
      characterCount: 0,
      startedAt: Date.now(),
      sessionId: controller.sessionId,
      controllerId: controller.key,
    } satisfies StreamAggregate);
  aggregate.deltaCount += 1;
  aggregate.characterCount += delta.length;
  streamAggregates.set(key, aggregate);
}

function flushStreamAggregate(runtimeId: string, generation: number): void {
  const key = streamAggregateKey(runtimeId, generation);
  const aggregate = streamAggregates.get(key);
  if (!aggregate) return;
  streamAggregates.delete(key);
  if (aggregate.deltaCount === 0) return;
  recordStreamAggregate(
    runtimeId,
    generation,
    aggregate.deltaCount,
    aggregate.characterCount,
    aggregate.startedAt,
    Date.now(),
    {
      sessionId: aggregate.sessionId,
      controllerId: aggregate.controllerId,
    },
  );
}

/**
 * Sends one Pi RPC request. The `pi.rpc` span starts here, the moment Tau
 * creates the request — not once `send_pi` finishes writing it — and ends
 * later on the exact matching response (`handleResponse`), a timeout, or
 * explicit abandonment. `parentContext` nests the span under an enclosing
 * `ui.action` span when this call is part of a traced user action.
 */
async function rpc(
  controller: SessionController,
  request: Record<string, unknown>,
  parentContext?: TraceContext,
): Promise<void> {
  const requestId = stringValue(request.id);
  const method = stringValue(request.type);
  const key = requestId
    ? rpcSpanKey(controller.runtimeId, controller.generation, requestId)
    : undefined;
  if (key) {
    const span = startRpcSpan(
      method as PiRpcMethod,
      requestId,
      controller.runtimeId,
      controller.generation,
      parentContext,
      {
        sessionId: controller.sessionId,
        controllerId: controller.key,
      },
    );
    registerPendingRpcSpan(key, span.end, span.context);
  }
  try {
    await invoke('send_pi', { runtimeId: controller.runtimeId, request });
  } catch (error) {
    if (key) endPendingRpcSpan(key, 'error');
    throw error;
  }
}

async function submitExtensionDialog(
  value: string | boolean,
  parentContext?: TraceContext,
): Promise<void> {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  const response =
    dialog.method === 'confirm' && typeof value === 'boolean'
      ? {
          type: 'extension_ui_response',
          id: dialog.requestId,
          confirmed: value,
        }
      : typeof value === 'string'
        ? { type: 'extension_ui_response', id: dialog.requestId, value }
        : {
            type: 'extension_ui_response',
            id: dialog.requestId,
            cancelled: true,
          };
  await respondToExtensionDialog(dialog, response, parentContext);
}

async function cancelExtensionDialog(
  parentContext?: TraceContext,
): Promise<void> {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  await respondToExtensionDialog(
    dialog,
    {
      type: 'extension_ui_response',
      id: dialog.requestId,
      cancelled: true,
    },
    parentContext,
  );
}

async function respondToExtensionDialog(
  dialog: ExtensionDialog,
  response: Record<string, unknown>,
  parentContext?: TraceContext,
): Promise<void> {
  if (dialog.submitting) return;
  const controller = controllerByKey(dialog.controllerKey);
  if (
    !controller ||
    controller.disposed ||
    controller.runtimeId !== dialog.runtimeId ||
    controller.generation !== dialog.generation
  ) {
    discardExtensionDialog(dialog.key);
    return;
  }

  dialog.submitting = true;
  dialog.error = '';
  try {
    await rpc(controller, response, parentContext);
    discardExtensionDialog(dialog.key);
    // An answered dialog is a common point for a workflow to open its next
    // session, and that swap is not reported by any event.
    watchSessionReplacement(controller);
  } catch {
    dialog.submitting = false;
    dialog.error = 'The response could not be sent. Try again.';
  }
}

function handleExtensionUIRequest(
  controller: SessionController,
  request: Record<string, unknown>,
): void {
  const requestId = stringValue(request.id);
  const method = stringValue(request.method);
  if (!requestId || !method) return;

  if (isExtensionDialogMethod(method)) {
    const origin = extensionRequestOrigin(controller);
    const timeout =
      typeof request.timeout === 'number' &&
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
      draft: typeof request.prefill === 'string' ? request.prefill : '',
      submitting: false,
      error: '',
      ...(typeof request.message === 'string'
        ? { message: request.message }
        : {}),
      ...(Array.isArray(request.options)
        ? {
            options: request.options.filter(
              (option): option is string => typeof option === 'string',
            ),
          }
        : {}),
      ...(typeof request.placeholder === 'string'
        ? { placeholder: request.placeholder }
        : {}),
      ...(typeof request.prefill === 'string'
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

  if (method === 'notify' && typeof request.message === 'string') {
    const id = `extension-notify:${extensionRequestKey(controller, requestId)}`;
    if (controller.messages.some((message) => message.id === id)) return;

    const origin = extensionRequestOrigin(controller);
    controller.messages.push({
      id,
      kind: 'notice',
      text: request.message,
      ...anchorFields(controller),
      noticeType: extensionNotifyType(request.notifyType),
      ...(origin.workingDirectory ? { basePath: origin.workingDirectory } : {}),
    });
    if (!isControllerSelected(controller)) controller.unread = true;
    return;
  }

  if (method === 'setStatus') return;

  if (method === 'set_editor_text' && typeof request.text === 'string') {
    controller.draft = request.text;
    const session = ephemeralSessionByController(controller.key);
    if (session?.phantom) session.title = draftTitle(request.text);
  }
}

function isExtensionDialogMethod(
  method: string,
): method is ExtensionDialogMethod {
  return (
    method === 'select' ||
    method === 'confirm' ||
    method === 'input' ||
    method === 'editor'
  );
}

function extensionDialogTitle(method: ExtensionDialogMethod): string {
  if (method === 'select') return 'Choose an option';
  if (method === 'confirm') return 'Confirm';
  if (method === 'input') return 'Enter a value';
  return 'Edit text';
}

function extensionNotifyType(value: unknown): TranscriptNoticeType {
  return value === 'warning' || value === 'error' ? value : 'info';
}

function extensionRequestOrigin(controller: SessionController): {
  workingDirectory?: string | undefined;
  projectName: string;
  sessionName: string;
} {
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
      'New session',
    // A remote project's files are on the other host, where nothing local can
    // open them, so its paths are left as text.
    ...(project && !project.connectionString
      ? { workingDirectory: project.workingDirectory }
      : {}),
  };
}

function extensionRequestKey(
  controller: SessionController,
  requestId: string,
): string {
  return `${controller.runtimeId}:${controller.generation}:${requestId}`;
}

function discardExtensionDialog(key: string): void {
  const timeout = extensionDialogTimeouts.get(key);
  if (timeout) clearTimeout(timeout);
  extensionDialogTimeouts.delete(key);
  const index = state.extensionDialogs.findIndex(
    (dialog) => dialog.key === key,
  );
  if (index >= 0) state.extensionDialogs.splice(index, 1);
}

function discardControllerDialogs(
  controller: SessionController,
  generation?: number,
): void {
  for (const dialog of [...state.extensionDialogs]) {
    if (
      dialog.controllerKey === controller.key &&
      (generation === undefined || dialog.generation === generation)
    ) {
      discardExtensionDialog(dialog.key);
    }
  }
}

function clearExtensionUiState(): void {
  for (const timeout of extensionDialogTimeouts.values()) clearTimeout(timeout);
  extensionDialogTimeouts.clear();
  state.extensionDialogs.splice(0);
}

const piConnectionFailureMessage =
  'The Pi connection failed. Select the session again to reconnect.';
const piProcessExitMessage =
  'The Pi process stopped unexpectedly. Select the session again to reconnect.';
const remotePiConnectionFailureMessage =
  'The remote Pi connection failed. Check the connection and try again.';
const remotePiProcessExitMessage =
  'The remote Pi process stopped unexpectedly. Check the connection and try again.';

async function handleBridgeEvent(event: PiBridgeEvent): Promise<void> {
  const controller = controllerByRuntimeId(event.runtimeId);
  if (!controller) return;
  touchController(controller);
  if (event.kind === 'started') {
    if (event.generation > controller.generation) {
      abandonPendingRpcSpans(
        controller.runtimeId,
        controller.generation,
        'abandoned_generation_change',
      );
      flushStreamAggregate(controller.runtimeId, controller.generation);
      discardControllerDialogs(controller);
      controller.generation = event.generation;
    }
    return;
  }
  if (event.generation !== controller.generation) return;

  if (event.kind === 'rpc' && event.line) {
    let value: unknown;
    try {
      value = JSON.parse(event.line);
    } catch {
      return;
    }
    await handleRpc(controller, value);
    return;
  }
  if (event.kind === 'stderr') return;
  if (event.kind === 'error') {
    const message = controller.connectingRemote
      ? remotePiConnectionFailureMessage
      : piConnectionFailureMessage;
    if (controller.connectingRemote) {
      presentRemoteConnectionError(controller, message);
      return;
    }
    if (controller.pendingPrompt) cancelPendingPrompt(controller, message);
    else setControllerError(controller, message);
    return;
  }
  if (event.kind === 'exited') {
    abandonPendingRpcSpans(
      controller.runtimeId,
      event.generation,
      'abandoned_process_exit',
    );
    flushStreamAggregate(controller.runtimeId, event.generation);
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    discardControllerDialogs(controller, event.generation);
    const message =
      event.code === 0
        ? ''
        : controller.connectingRemote
          ? remotePiProcessExitMessage
          : piProcessExitMessage;
    if (controller.connectingRemote) {
      presentRemoteConnectionError(
        controller,
        message || 'The remote Pi process stopped before it was ready.',
      );
      return;
    }
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message || 'The Pi process stopped.');
    }
    setControllerLifecycle(
      controller,
      {
        ready: false,
        streaming: false,
        stopping: false,
        starting: false,
        working: false,
      },
      'process_exited',
    );
    controller.runStateRequestId = '';
    controller.generation = 0;
    controller.status = message;
  }
}

async function handleRpc(
  controller: SessionController,
  value: unknown,
): Promise<void> {
  const event = asRecord(value);
  if (!event) return;
  const type = stringValue(event.type);
  if (type === 'extension_ui_request') {
    handleExtensionUIRequest(controller, event);
    return;
  }
  if (type === 'response') {
    await handleResponse(controller, event);
    return;
  }
  if (type === 'agent_start') {
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    resetStreamAggregate(controller.runtimeId, controller.generation);
    controller.localErrors = [];
    setControllerLifecycle(
      controller,
      { streaming: true, stopping: false, working: true },
      'agent_start',
    );
    controller.status = '';
    const stateRequestId = nextRequestId('run-state');
    controller.runStateRequestId = stateRequestId;
    await rpc(controller, { id: stateRequestId, type: 'get_state' });
    return;
  }
  if (type === 'message_start') {
    const message = asRecord(event.message);
    if (stringValue(message?.role) !== 'user') return;
    const skill = parseSkillBlock(contentText(message?.content));
    if (!skill) return;

    const optimistic = [...controller.messages]
      .reverse()
      .find(
        (entry) =>
          entry.kind === 'skill' &&
          entry.skillName === skill.name &&
          !entry.text,
      );
    if (optimistic) {
      optimistic.text = skill.content;
      optimistic.skillPrompt = skill.userMessage;
      return;
    }

    const invocation = `/skill:${skill.name}`;
    const rawOptimistic = [...controller.messages]
      .reverse()
      .find(
        (entry) =>
          entry.kind === 'user' &&
          entry.id.startsWith('optimistic-user-') &&
          (entry.text === invocation ||
            entry.text.startsWith(`${invocation} `)),
      );
    if (rawOptimistic) {
      const index = controller.messages.indexOf(rawOptimistic);
      controller.messages.splice(index, 1, {
        id: rawOptimistic.id,
        kind: 'skill',
        text: skill.content,
        skillName: skill.name,
        ...(skill.userMessage ? { skillPrompt: skill.userMessage } : {}),
      });
      return;
    }

    controller.messages.push({
      id: `stream-skill-${controller.streamSequence++}`,
      kind: 'skill',
      text: skill.content,
      skillName: skill.name,
      ...(skill.userMessage ? { skillPrompt: skill.userMessage } : {}),
    });
    return;
  }
  if (type === 'message_update') {
    const delta = asRecord(event.assistantMessageEvent);
    const deltaType = stringValue(delta?.type);
    const deltaText = stringValue(delta?.delta);
    if (deltaType === 'text_delta' || deltaType === 'thinking_delta') {
      recordStreamDelta(controller, deltaText);
    }
    if (deltaType === 'text_delta') {
      appendStream(controller, 'assistant', deltaText);
      if (!isControllerSelected(controller)) controller.unread = true;
    }
    if (deltaType === 'thinking_delta') {
      appendStream(controller, 'thinking', deltaText);
    }
    return;
  }
  // A turn that failed is settled from Pi's messages soon after, but only once
  // the run ends, and a retry can hold that off for the length of its backoff.
  if (type === 'message_end') {
    const failure = messageFailure(event.message);
    if (failure) pushError(controller, failure);
    return;
  }
  /**
   * A failed compaction is reported once and kept nowhere: Pi's message list
   * has no entry for it, so the row is held on the controller and re-appended
   * to every list hydrated until the next run begins.
   */
  if (type === 'compaction_end') {
    const failure = stringValue(event.errorMessage);
    if (failure) {
      const error = {
        key: controller.streamSequence++,
        text: failure,
        ...anchorFields(controller),
      };
      controller.localErrors.push(error);
      // The row shown now and the one merged back later are the same row: it
      // carries the failure's own id and spot rather than passing for a Pi row,
      // which would both stall id adoption and count towards later anchors.
      controller.messages.push({
        id: localErrorId(error.key),
        kind: 'error',
        text: failure,
        ...(error.anchor === undefined ? {} : { anchor: error.anchor }),
      });
      if (!isControllerSelected(controller)) controller.unread = true;
    }
    return;
  }
  if (type === 'tool_execution_start') {
    const toolCallId = stringValue(event.toolCallId);
    const toolName = stringValue(event.toolName) || 'tool';
    controller.messages.push({
      id: `stream-tool-${controller.streamSequence++}`,
      kind: 'tool',
      text: toolSummary(event.args),
      toolCallId,
      toolName,
      toolRunning: true,
      toolErrored: false,
      toolArguments: toolArgumentsText(event.args),
    });
    return;
  }
  if (type === 'tool_execution_end') {
    const toolCallId = stringValue(event.toolCallId);
    const tool = [...controller.messages]
      .reverse()
      .find(
        (entry) => entry.kind === 'tool' && entry.toolCallId === toolCallId,
      );
    if (tool) {
      tool.toolRunning = false;
      tool.toolErrored = event.isError === true;
      tool.toolResult = toolResultText(asRecord(event.result)?.content);
    }
    return;
  }
  if (type === 'agent_settled') {
    flushStreamAggregate(controller.runtimeId, controller.generation);
    clearAbortWatch(controller);
    setControllerLifecycle(
      controller,
      { syncing: true, working: false, streaming: false, stopping: false },
      'agent_settled',
    );
    controller.status = '';
    if (!isControllerSelected(controller)) controller.unread = true;
    watchSessionReplacement(controller);
    await rpc(controller, {
      id: nextRequestId('settled-state'),
      type: 'get_state',
    });
    return;
  }
  // Pi announces every rename, whether it came from Tau's header, one of Pi's
  // own commands, or an extension, so the name is only ever read back from Pi.
  if (type === 'session_info_changed') {
    applySessionName(controller, stringValue(event.name));
    await persistSessionName(controller);
    return;
  }
  if (type === 'auto_retry_start') {
    controller.status = retryStatus(event);
    return;
  }
  if (type === 'extension_error') {
    controller.status = stringValue(event.error) || 'A Pi extension failed.';
  }
}

async function handleResponse(
  controller: SessionController,
  response: Record<string, unknown>,
): Promise<void> {
  const command = stringValue(response.command);
  const responseId = stringValue(response.id);
  const pendingResult = responseId
    ? endPendingRpcSpan(
        rpcSpanKey(controller.runtimeId, controller.generation, responseId),
        response.success === true ? 'success' : 'error',
      )
    : { matched: false };
  if (responseId && !pendingResult.matched) {
    recordRpcResponseAnomaly('unmatched_or_duplicate', responseId, {
      sessionId: controller.sessionId,
      controllerId: controller.key,
      runtimeId: controller.runtimeId,
      generation: controller.generation,
    });
  }
  const responseContext = pendingResult.context;
  const resolvesRunState =
    command === 'get_state' &&
    Boolean(controller.runStateRequestId) &&
    responseId === controller.runStateRequestId;
  if (resolvesRunState) controller.runStateRequestId = '';
  const resolvesReplacementProbe =
    command === 'get_state' &&
    Boolean(controller.replacementProbeRequestId) &&
    responseId === controller.replacementProbeRequestId;
  if (resolvesReplacementProbe) controller.replacementProbeRequestId = '';
  const resolvesCommandSync =
    command === 'get_state' &&
    Boolean(controller.commandSyncRequestId) &&
    responseId === controller.commandSyncRequestId;
  if (resolvesCommandSync) controller.commandSyncRequestId = '';
  const resolvesAbortProbe =
    command === 'get_state' &&
    Boolean(controller.abortProbeRequestId) &&
    responseId === controller.abortProbeRequestId;
  if (resolvesAbortProbe) controller.abortProbeRequestId = '';
  if (
    command === 'prompt' &&
    Boolean(controller.commandPromptRequestId) &&
    responseId === controller.commandPromptRequestId
  ) {
    controller.commandPromptRequestId = '';
    if (response.success === true) {
      setControllerLifecycle(
        controller,
        { working: controller.streaming },
        'prompt_response',
        responseContext,
      );
      await syncAfterCommand(controller);
      return;
    }
  }
  if (response.success !== true) {
    const error = asRecord(response.error);
    controller.status =
      stringValue(error?.message) ||
      stringValue(response.error) ||
      `Pi rejected ${command || 'the request'}.`;
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
      ((command === 'get_state' && responseId === pending?.stateRequestId) ||
        (command === 'get_messages' &&
          responseId === pending?.messagesRequestId));
    if (failedPendingRequest) {
      cancelPendingPrompt(controller, controller.status);
    }
    if (command === 'get_state') {
      const resolvesBootstrap =
        responseId === controller.bootstrapStateRequestId;
      if (resolvesBootstrap) controller.bootstrapStateRequestId = '';
      setControllerLifecycle(
        controller,
        resolvesBootstrap
          ? { syncing: false, starting: false }
          : { syncing: false },
        'get_state_failed',
        responseContext,
      );
      releaseRuntime(controller);
    }
    if (
      command === 'get_messages' &&
      responseId === controller.startMessagesRequestId
    ) {
      controller.startMessagesRequestId = '';
      setControllerLifecycle(
        controller,
        { starting: false, syncing: false },
        'get_messages_failed',
        responseContext,
      );
    }
    if (command === 'prompt') {
      setControllerLifecycle(
        controller,
        { working: false },
        'prompt_failed',
        responseContext,
      );
    }
    if (command === 'abort') {
      setControllerLifecycle(
        controller,
        { stopping: false },
        'abort_failed',
        responseContext,
      );
    }
    // Restore the last confirmed title immediately, then ask Pi in case an
    // extension changed the name while this request was in flight.
    if (command === 'set_session_name') {
      const pending = controller.pendingSessionRename;
      if (pending?.requestId === responseId) {
        applySessionName(
          controller,
          pending.previousName,
          pending.previousTitle,
        );
        controller.pendingSessionRename = undefined;
      }
      await rpc(controller, {
        id: nextRequestId('session-name-state'),
        type: 'get_state',
      });
    }
    return;
  }
  if (
    command === 'set_session_name' &&
    controller.pendingSessionRename?.requestId === responseId
  ) {
    controller.pendingSessionRename = undefined;
  }
  const data = asRecord(response.data);

  if (command === 'get_state' && data) {
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
    applySessionName(controller, stringValue(data.sessionName));
    const nowStreaming = data.isStreaming === true;
    controller.status =
      resolvesAbortProbe && nowStreaming ? unstoppedStatus(controller) : '';
    finishRemoteConnection(controller);

    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom && !unsavedSession) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }

    let syncingAfterSessionChange = false;
    if (sessionChanged) {
      // The response that revealed the replacement already ended its own
      // span above; anything else still pending for this runtime and
      // generation belongs to the session Pi just swapped out from under it.
      abandonPendingRpcSpans(
        controller.runtimeId,
        controller.generation,
        'abandoned_replacement',
      );
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      rebindEphemeralSession(controller);
      controller.messages = [];
      controller.models = [];
      controller.efforts = [];
      controller.commands = [];
      controller.commandsLoaded = false;
      controller.pendingEffort = '';
      syncingAfterSessionChange = true;
    }
    setControllerLifecycle(
      controller,
      {
        ready: true,
        streaming: nowStreaming,
        stopping: false,
        working: nowStreaming || Boolean(pending),
        connectingRemote: false,
        ...(syncingAfterSessionChange ? { syncing: true } : {}),
      },
      'get_state_response',
      responseContext,
    );

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
      // Pi hands Tau an identity when it replaces the session, when it names
      // the session a prompt just created, and when it answers for the
      // session an extension command left behind. Bootstraps and run-state
      // polls only re-state a session Tau already opened.
      await registerConnectedSession(
        controller,
        undefined,
        sessionChanged || Boolean(resolvesPending) || resolvesCommandSync,
      );
    }
    if (controller.disposed) return;

    if (resolvesPending && pending) {
      controller.bootstrapStateRequestId = '';
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
        id: nextRequestId('replacement-models'),
        type: 'get_available_models',
      });
      await rpc(controller, {
        id: nextRequestId('replacement-commands'),
        type: 'get_commands',
      });
    }

    const messagesRequestId = nextRequestId('messages');
    if (resolvesBootstrap) {
      controller.startMessagesRequestId = messagesRequestId;
      controller.bootstrapStateRequestId = '';
    }
    await rpc(controller, {
      id: nextRequestId('efforts'),
      type: 'get_available_thinking_levels',
    });
    await rpc(controller, { id: messagesRequestId, type: 'get_messages' });
    return;
  }

  if (command === 'get_messages' && data) {
    const previous = controller.messages;
    controller.messagesLoaded = true;
    controller.messages = mergeLocalEntries(
      hydrateTranscript(
        Array.isArray(data.messages) ? data.messages : [],
        previous,
      ),
      controller.localErrors,
      previous,
    );
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.messagesRequestId) &&
      responseId === pending?.messagesRequestId;
    const resolvesStart =
      !resolvesPending && responseId === controller.startMessagesRequestId;
    if (resolvesStart) controller.startMessagesRequestId = '';
    setControllerLifecycle(
      controller,
      resolvesStart ? { syncing: false, starting: false } : { syncing: false },
      'get_messages_response',
      responseContext,
    );
    if (resolvesPending) {
      await dispatchPendingPrompt(controller);
    }
    // A hidden session that finishes hydrating has nothing left to wait for,
    // so this is where a runtime the user has moved on from is accounted for.
    releaseIdleRuntimes();
    return;
  }

  if (command === 'get_available_models' && data) {
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

  if (command === 'get_available_thinking_levels' && data) {
    controller.efforts = (Array.isArray(data.levels) ? data.levels : [])
      .map(normalizeEffort)
      .filter((level, index, levels) => levels.indexOf(level) === index);
    return;
  }

  if (command === 'get_commands' && data) {
    controller.commands = (Array.isArray(data.commands) ? data.commands : [])
      .map(commandOption)
      .filter((command): command is CommandOption => Boolean(command));
    controller.commandsLoaded = true;
    return;
  }

  if (command === 'prompt') {
    setControllerLifecycle(
      controller,
      { working: controller.streaming },
      'prompt_response',
      responseContext,
    );
    return;
  }

  if (command === 'set_model') {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === 'model' &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = '';
      pending.settingsStep = '';
      controller.currentModelProvider = pending.selectedModelProvider;
      controller.currentModelId = pending.selectedModelId;
      controller.currentModelName = pending.selectedModelName;
      await applyPendingSessionEffort(controller, true);
      return;
    }
    await rpc(controller, {
      id: nextRequestId('model-state'),
      type: 'get_state',
    });
    return;
  }

  if (command === 'set_thinking_level') {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === 'effort' &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = '';
      pending.settingsStep = '';
      controller.currentEffort = pending.selectedEffort;
      controller.status = '';
      await requestPendingMessages(controller);
      return;
    }
    if (controller.pendingEffort) {
      controller.currentEffort = controller.pendingEffort;
    }
    controller.pendingEffort = '';
    controller.status = '';
  }
}

async function applyPendingSessionSettings(
  controller: SessionController,
): Promise<void> {
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

  const requestId = nextRequestId('initial-model');
  pending.settingsRequestId = requestId;
  pending.settingsStep = 'model';
  await rpc(
    controller,
    {
      id: requestId,
      type: 'set_model',
      provider: pending.selectedModelProvider,
      modelId: pending.selectedModelId,
    },
    pending.telemetryContext,
  );
}

async function applyPendingSessionEffort(
  controller: SessionController,
  force: boolean,
): Promise<void> {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  if (!force && pending.selectedEffort === controller.currentEffort) {
    await requestPendingMessages(controller);
    return;
  }

  const requestId = nextRequestId('initial-effort');
  pending.settingsRequestId = requestId;
  pending.settingsStep = 'effort';
  await rpc(
    controller,
    {
      id: requestId,
      type: 'set_thinking_level',
      level: pending.selectedEffort,
    },
    pending.telemetryContext,
  );
}

async function requestPendingMessages(
  controller: SessionController,
): Promise<void> {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  const messagesRequestId = nextRequestId('messages');
  pending.messagesRequestId = messagesRequestId;
  await rpc(
    controller,
    { id: nextRequestId('efforts'), type: 'get_available_thinking_levels' },
    pending.telemetryContext,
  );
  await rpc(
    controller,
    { id: messagesRequestId, type: 'get_messages' },
    pending.telemetryContext,
  );
}

async function dispatchPendingPrompt(
  controller: SessionController,
): Promise<void> {
  const prompt = controller.pendingPrompt;
  if (!prompt) return;
  controller.pendingPrompt = undefined;
  setControllerLifecycle(
    controller,
    { starting: false, working: true },
    'pending_prompt_dispatch',
    prompt.telemetryContext,
  );
  if (
    !prompt.command &&
    !controller.messages.some((message) => message.id === prompt.optimisticId)
  ) {
    appendOptimisticPrompt(controller, prompt.message, prompt.optimisticId);
  }
  try {
    const requestId = nextRequestId('prompt');
    if (prompt.command) controller.commandPromptRequestId = requestId;
    await rpc(
      controller,
      { id: requestId, type: 'prompt', message: prompt.message },
      prompt.telemetryContext,
    );
    controller.promptSubmitting = false;
    if (controller.draft === prompt.message) controller.draft = '';
  } catch (error) {
    controller.promptSubmitting = false;
    setControllerLifecycle(
      controller,
      { working: false },
      'pending_prompt_failed',
      prompt.telemetryContext,
    );
    controller.commandPromptRequestId = '';
    if (!controller.draft || controller.draft === prompt.message) {
      controller.draft = prompt.message;
    }
    setControllerError(controller, error);
  }
}

async function syncAfterCommand(controller: SessionController): Promise<void> {
  if (controller.disposed || !controller.generation) return;
  const requestId = nextRequestId('command-sync');
  controller.commandSyncRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch (error) {
    controller.commandSyncRequestId = '';
    setControllerError(controller, error);
  }
}

function watchSessionReplacement(controller: SessionController): void {
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

async function probeSessionReplacement(
  controller: SessionController,
  last: boolean,
): Promise<void> {
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
  const requestId = nextRequestId('replacement-probe');
  controller.replacementProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch {
    controller.replacementProbeRequestId = '';
  }
}

function clearSessionReplacementWatch(controller: SessionController): void {
  const timers = replacementProbeTimers.get(controller.key);
  if (!timers) return;
  replacementProbeTimers.delete(controller.key);
  for (const timer of timers) clearTimeout(timer);
}

function watchingSessionReplacement(controller: SessionController): boolean {
  return replacementProbeTimers.has(controller.key);
}

function watchAbort(controller: SessionController): void {
  clearAbortWatch(controller);
  if (controller.disposed || !controller.generation) return;
  abortProbeTimers.set(
    controller.key,
    setTimeout(() => {
      void probeAbort(controller);
    }, abortAcknowledgeDelay),
  );
}

async function probeAbort(controller: SessionController): Promise<void> {
  abortProbeTimers.delete(controller.key);
  if (controller.disposed || !controller.generation || !controller.stopping) {
    return;
  }
  const requestId = nextRequestId('abort-probe');
  controller.abortProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch (error) {
    controller.abortProbeRequestId = '';
    setControllerLifecycle(
      controller,
      { stopping: false },
      'abort_probe_failed',
    );
    setControllerError(controller, error);
  }
}

function clearAbortWatch(controller: SessionController): void {
  const timer = abortProbeTimers.get(controller.key);
  if (!timer) return;
  abortProbeTimers.delete(controller.key);
  clearTimeout(timer);
}

function unstoppedStatus(controller: SessionController): string {
  const tool = [...controller.messages]
    .reverse()
    .find((entry) => entry.kind === 'tool' && entry.toolRunning);
  return tool?.toolName
    ? `Pi is still running ${tool.toolName} and stops once it returns.`
    : 'Pi has not stopped yet and stops once its current work finishes.';
}

function appendOptimisticPrompt(
  controller: SessionController,
  message: string,
  id: string,
): void {
  const invocation = skillInvocation(controller, message);
  if (!invocation) {
    controller.messages.push({ id, kind: 'user', text: message });
    return;
  }

  controller.messages.push({
    id,
    kind: 'skill',
    text: '',
    skillName: invocation.name,
    ...(invocation.userMessage ? { skillPrompt: invocation.userMessage } : {}),
  });
}

function skillInvocation(
  controller: SessionController,
  message: string,
): { name: string; userMessage: string } | undefined {
  if (!message.startsWith('/')) return undefined;
  const space = message.indexOf(' ');
  const commandName = space < 0 ? message.slice(1) : message.slice(1, space);
  const command = controller.commands.find(
    (candidate) =>
      candidate.name === commandName && candidate.source === 'skill',
  );
  if (!command) return undefined;
  return {
    name: commandName.slice('skill:'.length),
    userMessage: space < 0 ? '' : message.slice(space + 1).trim(),
  };
}

function invokesExtensionCommand(
  controller: SessionController,
  message: string,
): boolean {
  if (!message.startsWith('/')) return false;
  // Pi takes the command name from the first space, so anything else is a
  // prompt for the model even when it opens with a slash.
  const space = message.indexOf(' ');
  const name = space < 0 ? message.slice(1) : message.slice(1, space);
  return (
    Boolean(name) &&
    controller.commands.some(
      (command) => command.name === name && command.source === 'extension',
    )
  );
}

function applySessionName(
  controller: SessionController,
  name: string,
  fallbackTitle = '',
): void {
  controller.sessionName = name;
  const title = name || fallbackTitle;
  if (!title) return;

  const ephemeral = ephemeralSessionByController(controller.key);
  if (ephemeral && !ephemeral.phantom) {
    ephemeral.title = title;
    return;
  }

  const project = state.workspace?.projects.find(
    (candidate) => candidate.path === controller.projectPath,
  );
  const session = project?.sessions.find(
    (candidate) => candidate.id === controller.sessionId,
  );
  if (session) session.title = title;
}

async function persistSessionName(
  controller: SessionController,
): Promise<void> {
  if (controller.phantom || !controller.sessionId || !controller.sessionPath) {
    return;
  }
  await registerConnectedSession(controller);
}

async function registerConnectedSession(
  controller: SessionController,
  parentContext?: TraceContext,
  // Set when Pi handed Tau this identity rather than Tau opening a session it
  // already listed: adopting a session means its row must exist and be
  // reachable, even when Tau had archived it before Pi handed it back.
  adopted = false,
): Promise<void> {
  try {
    const workspace = await registerSession(controller, adopted, parentContext);
    if (controller.disposed) return;
    state.workspace = workspace;
    if (workspaceContainsSession(controller)) {
      removeRegisteredEphemeralSession(controller);
    }
    if (!isControllerSelected(controller)) return;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.workspace = await selectRegisteredSession(controller, parentContext);
  } catch (error) {
    setControllerError(controller, error);
  }
}

function registerSession(
  controller: SessionController,
  adopted: boolean,
  parentContext?: TraceContext,
): Promise<WorkspaceSnapshot> {
  return invokeTraced<WorkspaceSnapshot>(
    'register_session',
    {
      projectPath: controller.projectPath,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
      sessionName:
        controller.sessionName || firstUserMessage(controller) || null,
      lastUserMessageAt:
        controller.lastUserMessageAt > 0 ? controller.lastUserMessageAt : null,
      adopted,
    },
    parentContext,
  );
}

// A session Tau is showing has to be selectable. Tau's registry rejecting it
// means the row and the live runtime disagree, so adopt the session Pi is
// holding and select it once more rather than leaving it unreachable.
async function selectRegisteredSession(
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<WorkspaceSnapshot> {
  const select = (): Promise<WorkspaceSnapshot> =>
    invokeTraced<WorkspaceSnapshot>(
      'set_active_session',
      {
        projectPath: controller.projectPath,
        sessionId: controller.sessionId,
      },
      parentContext,
    );
  try {
    return await select();
  } catch (error) {
    if (
      controller.phantom ||
      !controller.sessionId ||
      !controller.sessionPath
    ) {
      throw error;
    }
    state.workspace = await registerSession(controller, true, parentContext);
    return await select();
  }
}

async function persistExpandedProject(
  projectPath: string,
  parentContext?: TraceContext,
): Promise<void> {
  try {
    state.workspace = await invokeTraced<WorkspaceSnapshot>(
      'set_project_collapsed',
      { path: projectPath, collapsed: false },
      parentContext,
    );
  } catch (error) {
    setActiveError(error);
  }
}

async function persistProjectSelection(
  projectPath: string,
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<void> {
  try {
    if (controller.phantom) {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'set_active_project',
        { path: projectPath },
        parentContext,
      );
    } else {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'set_active_session',
        { projectPath, sessionId: controller.sessionId },
        parentContext,
      );
    }
  } catch (error) {
    setControllerError(controller, error);
  }
}

// An unregistered row keeps a session reachable while Tau's registry does
// not, so a replacement has to move it onto the identity Pi handed over
// instead of leaving it pointing at the session that was swapped out.
function rebindEphemeralSession(controller: SessionController): void {
  const session = ephemeralSessionByController(controller.key);
  if (!session) return;
  session.id = controller.sessionId;
  session.path = controller.sessionPath;
}

function materializePendingSession(
  controller: SessionController,
  sessionId: string,
  sessionPath: string,
): void {
  const session = ephemeralSessionByController(controller.key);
  controller.sessionId = sessionId;
  controller.sessionPath = sessionPath;
  controller.phantom = false;
  if (session) {
    session.id = sessionId;
    session.path = sessionPath;
    session.phantom = false;
    session.title = draftTitle(controller.pendingPrompt?.message ?? '');
  }
  if (isControllerSelected(controller)) {
    state.activeSessionId = sessionId;
    state.activeSessionPath = sessionPath;
  }
}

function cancelPendingPrompt(
  controller: SessionController,
  error: unknown,
): void {
  const prompt = controller.pendingPrompt;
  if (!prompt) {
    setControllerError(controller, error);
    return;
  }
  controller.pendingPrompt = undefined;
  controller.promptSubmitting = false;
  setControllerLifecycle(
    controller,
    { starting: false, working: false },
    'pending_prompt_cancelled',
    prompt.telemetryContext,
  );
  controller.messages = controller.messages.filter(
    (message) => message.id !== prompt.optimisticId,
  );
  if (!controller.draft || controller.draft === prompt.message) {
    controller.draft = prompt.message;
  }
  setControllerError(controller, error);
}

/**
 * The spot a local entry has to hold on to so a rebuild from Pi's messages puts
 * it back where it was: the number of transcript rows Pi owns right now, zero
 * of them included. A session whose transcript has not loaded yet cannot tell a
 * session without history from one whose history has yet to arrive, and an entry
 * anchored above history it actually followed would be worse than one left at
 * the bottom, so only that case claims no spot at all.
 */
function anchorFields(controller: SessionController): { anchor?: number } {
  if (!controller.messagesLoaded) return {};
  return {
    anchor: controller.messages.filter(
      (message) => message.kind !== 'notice' && message.anchor === undefined,
    ).length,
  };
}

function pushError(controller: SessionController, text: string): void {
  controller.messages.push({
    id: `stream-error-${controller.streamSequence++}`,
    kind: 'error',
    text,
  });
  if (!isControllerSelected(controller)) controller.unread = true;
}

/** How much of a reason a retry line carries before the composer is pushed down. */
const retryReasonLimit = 120;

function retryStatus(event: Record<string, unknown>): string {
  const attempt = String(event.attempt ?? '');
  const maxAttempts = event.maxAttempts ? `/${String(event.maxAttempts)}` : '';
  const reason = describePiError(stringValue(event.errorMessage)).message;
  const trimmed =
    reason.length > retryReasonLimit
      ? `${reason.slice(0, retryReasonLimit).trimEnd()}…`
      : reason;
  return trimmed
    ? `Retrying (${attempt}${maxAttempts}): ${trimmed}`
    : `Retrying (${attempt}${maxAttempts})…`;
}

function appendStream(
  controller: SessionController,
  kind: 'assistant' | 'thinking',
  delta: string,
): void {
  if (!delta) return;
  const last = controller.messages[controller.messages.length - 1];
  if (last?.kind === kind && last.id.startsWith('stream-')) {
    last.text += delta;
    return;
  }
  controller.messages.push({
    id: `stream-${kind}-${controller.streamSequence++}`,
    kind,
    text: delta,
  });
}

async function retireUnsavedSession(
  controller: SessionController,
): Promise<void> {
  const staleSessionId = controller.sessionId;
  if (workspaceContainsSession(controller)) {
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'archive_session',
        { projectPath: controller.projectPath, sessionId: staleSessionId },
      );
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
  controller.sessionPath = '';
  controller.sessionName = '';
  controller.lastUserMessageAt = 0;
  controller.messages = [];
  if (isControllerSelected(controller)) {
    state.activeSessionId = session.id;
    state.activeSessionPath = '';
  }
  controller.status =
    'That session was never saved by Pi, so this is a new one.';
}

function removeEmptyActivePhantom(): void {
  const controller = activeController.value;
  const session = controller
    ? ephemeralSessionByController(controller.key)
    : undefined;
  // Pi gives a phantom its real in-memory identity before executing an
  // extension command. A command that never answers can therefore leave an
  // empty, unregistered row that is no longer marked phantom. It is just as
  // disposable as the unsent row it came from when the user leaves it.
  const unansweredUnsavedCommand = Boolean(
    controller &&
    session &&
    !session.phantom &&
    controller.commandPromptRequestId &&
    !controller.streaming &&
    !workspaceContainsSession(controller),
  );
  if (
    !controller ||
    !session ||
    (!session.phantom && !unansweredUnsavedCommand) ||
    controller.draft.trim() ||
    (controller.working && !unansweredUnsavedCommand) ||
    controllerHasPendingDialog(controller) ||
    controller.messages.length > 0
  ) {
    return;
  }
  removeEphemeralSession(session, true);
}

function discardUnregisteredEphemeralSession(
  controller: SessionController,
): boolean {
  const session = ephemeralSessionByController(controller.key);
  if (!session || workspaceContainsSession(controller)) return false;
  removeEphemeralSession(session, true);
  return true;
}

function removeRegisteredEphemeralSession(controller: SessionController): void {
  const session = ephemeralSessionByController(controller.key);
  if (session && !session.phantom) removeEphemeralSession(session, false);
}

function removeEphemeralSession(
  session: EphemeralSession,
  removeController: boolean,
): void {
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

function removeProjectUiState(projectPath: string): void {
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

function canReleaseRuntime(controller: SessionController): boolean {
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

function releaseRuntime(controller: SessionController | undefined): void {
  if (!controller || !canReleaseRuntime(controller)) return;
  void stopControllerProcess(controller);
}

function releaseIdleRuntimes(): void {
  const idle = state.controllers
    .filter((controller) => canReleaseRuntime(controller))
    .sort(
      (first, second) => second.lastActiveSequence - first.lastActiveSequence,
    );
  for (const controller of idle.slice(idleRuntimeLimit)) {
    void stopControllerProcess(controller);
  }
}

async function stopControllerProcess(
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<void> {
  if (!controller.generation && !controller.starting) return;
  const generation = controller.generation;
  try {
    await invokeTraced(
      'stop_pi',
      { runtimeId: controller.runtimeId },
      parentContext,
    );
  } catch (error) {
    setControllerError(controller, error);
    return;
  }
  if (controller.generation !== generation) return;
  clearSessionReplacementWatch(controller);
  clearAbortWatch(controller);
  discardControllerDialogs(controller, generation);
  abandonPendingRpcSpans(controller.runtimeId, generation, 'abandoned_stop');
  flushStreamAggregate(controller.runtimeId, generation);
  controller.generation = 0;
  setControllerLifecycle(
    controller,
    {
      ready: false,
      streaming: false,
      stopping: false,
      starting: false,
      working: false,
      connectingRemote: false,
      syncing: false,
    },
    'process_stopped',
    parentContext,
  );
  controller.runStateRequestId = '';
  controller.abortProbeRequestId = '';

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

export {
  sendPhantomMessage,
  startController,
  refreshModelScope,
  requestBootstrap,
  rpc,
  submitExtensionDialog,
  cancelExtensionDialog,
  respondToExtensionDialog,
  handleExtensionUIRequest,
  isExtensionDialogMethod,
  extensionDialogTitle,
  extensionNotifyType,
  extensionRequestOrigin,
  extensionRequestKey,
  discardExtensionDialog,
  discardControllerDialogs,
  clearExtensionUiState,
  handleBridgeEvent,
  handleRpc,
  handleResponse,
  applyPendingSessionSettings,
  applyPendingSessionEffort,
  requestPendingMessages,
  dispatchPendingPrompt,
  syncAfterCommand,
  watchSessionReplacement,
  probeSessionReplacement,
  clearSessionReplacementWatch,
  watchingSessionReplacement,
  watchAbort,
  probeAbort,
  clearAbortWatch,
  unstoppedStatus,
  appendOptimisticPrompt,
  skillInvocation,
  invokesExtensionCommand,
  applySessionName,
  persistSessionName,
  registerConnectedSession,
  persistExpandedProject,
  persistProjectSelection,
  materializePendingSession,
  cancelPendingPrompt,
  pushError,
  retryReasonLimit,
  retryStatus,
  appendStream,
  retireUnsavedSession,
  removeEmptyActivePhantom,
  discardUnregisteredEphemeralSession,
  removeRegisteredEphemeralSession,
  removeEphemeralSession,
  removeProjectUiState,
  canReleaseRuntime,
  releaseRuntime,
  releaseIdleRuntimes,
  stopControllerProcess,
  pendingRpcCount,
  oldestPendingRpcAgeMs,
  piConnectionFailureMessage,
  piProcessExitMessage,
};
