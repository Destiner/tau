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
  setControllerError,
  setWorkspaceError,
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
import { errorCopy, rpcFailureCopy } from '../error-copy';
import {
  invokeTraced,
  recordRpcResponseAnomaly,
  recordStreamAggregate,
  startRpcSpan,
} from '../telemetry';
import type { PiRpcMethod, PiRpcOutcome } from '../telemetry/attributes';
import type { TraceContext } from '../telemetry/trace-context';

import type { PiBridgeEvent } from './bridge';
import { describePiError, retryPiErrorMessage } from './error';
import type { ModelOption } from './model-scope';
import {
  asRecord,
  contentText,
  hydrateTranscript,
  historyLayersFromEntries,
  historyPrefix,
  localErrorId,
  mergeLocalEntries,
  messageFailure,
  parseSkillBlock,
  projectOrdinaryUserMessage,
  stringValue,
  toolArgumentsText,
  toolResultText,
  toolSummary,
  type TranscriptNoticeType,
} from './transcript';

const materializationVerificationRetryDelays = [250, 750, 1_500];

interface MaterializationVerificationRetry {
  generation: number;
  sessionId: string;
  sessionPath: string;
  nextAttempt: number;
  timer?: ReturnType<typeof setTimeout>;
}

const materializationVerificationRetries = new Map<
  string,
  MaterializationVerificationRetry
>();

async function sendPhantomMessage(
  controller: SessionController,
  message: string,
  draft: string,
  command: boolean,
  parentContext?: TraceContext,
): Promise<void> {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project || !controller.phantom) {
    controller.promptSubmitting = false;
    restoreSubmittedDraft(controller, draft);
    setControllerError(controller, errorCopy.messageSend);
    return;
  }

  const optimisticId = `optimistic-user-${Date.now()}`;
  controller.pendingPrompt = {
    message,
    draft,
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
    } catch {
      cancelPendingPrompt(controller, errorCopy.messageSend);
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
      working: Boolean(controller.pendingPrompt),
      connectingRemote: Boolean(project.connectionString),
    },
    'controller_start',
    parentContext,
  );
  controller.compacting = false;
  controller.bootstrapStateRequestId = '';
  controller.bootstrapSessionPath = sessionPath ?? '';
  controller.runStateRequestId = '';
  controller.startMessagesRequestId = '';
  controller.historyRequestId = '';
  setHistoryLoading(controller, false);
  controller.commandPromptRequestId = '';
  controller.commandSyncRequestId = '';
  controller.replacementProbeRequestId = '';
  controller.postSettlementHydration = false;
  controller.settledAssistantActivity = false;
  controller.materializationBarrierRequestId = '';
  controller.materializationStateRequestId = '';
  controller.materializationMessagesRequestId = '';
  controller.pendingSessionRename = undefined;
  settleInterruptedSubmittedPrompt(controller);
  controller.pendingEffort = '';
  controller.pendingSettingRequestId = '';
  controller.abortProbeRequestId = '';
  controller.remoteConnectionTimedOut = false;
  clearSettingRequestWatch(controller);
  touchController(controller);
  clearSessionReplacementWatch(controller);
  clearMaterializationVerificationWatch(controller);
  clearAbortWatch(controller);
  clearRemoteConnectionWatch(controller);
  if (project.connectionString) watchRemoteConnection(controller);
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
    if (
      controller.disposed ||
      (project.connectionString && !controller.connectingRemote)
    ) {
      await stopControllerProcess(controller, parentContext, false);
      return;
    }
    await requestBootstrap(controller, parentContext);
  } catch {
    clearRemoteConnectionWatch(controller);
    setControllerLifecycle(
      controller,
      { starting: false, connectingRemote: false, syncing: false },
      'controller_start_failed',
      parentContext,
    );
    const message = project.connectionString
      ? errorCopy.remoteConnection
      : errorCopy.piStart;
    if (controller.pendingPrompt) cancelPendingPrompt(controller, message);
    if (project.connectionString)
      presentRemoteConnectionError(controller, message);
    else setControllerError(controller, message);
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
  dispatchSnapshot: RpcDispatchSnapshot;
  method: string;
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
const confirmedAdmissionRequestIds = new Set<string>();

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
  dispatchSnapshot: RpcDispatchSnapshot,
  method: string,
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
    dispatchSnapshot,
    method,
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
  dispatchSnapshot?: RpcDispatchSnapshot;
  method?: string;
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
  return {
    matched: true,
    context: pending.context,
    dispatchSnapshot: pending.dispatchSnapshot,
    method: pending.method,
  };
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
interface RpcDispatchSnapshot {
  generation: number;
  sessionId: string;
  sessionPath: string;
  materializationBarrierRequestId: string;
  materializationStateRequestId: string;
  materializationMessagesRequestId: string;
  pendingPrompt: SessionController['pendingPrompt'];
  submittedPrompt: SessionController['submittedPrompt'];
}

function captureRpcDispatchSnapshot(
  controller: SessionController,
): RpcDispatchSnapshot {
  return {
    generation: controller.generation,
    sessionId: controller.sessionId,
    sessionPath: controller.sessionPath,
    materializationBarrierRequestId: controller.materializationBarrierRequestId,
    materializationStateRequestId: controller.materializationStateRequestId,
    materializationMessagesRequestId:
      controller.materializationMessagesRequestId,
    pendingPrompt: controller.pendingPrompt,
    submittedPrompt: controller.submittedPrompt,
  };
}

function rpcDispatchStillCurrent(
  controller: SessionController,
  snapshot: RpcDispatchSnapshot,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === snapshot.generation &&
    controller.sessionId === snapshot.sessionId &&
    controller.sessionPath === snapshot.sessionPath
  );
}

function cleanupRejectedRpcDispatch(
  controller: SessionController,
  requestId: string,
  method: string,
  snapshot: RpcDispatchSnapshot,
): void {
  if (!rpcDispatchStillCurrent(controller, snapshot)) return;

  if (controller.bootstrapStateRequestId === requestId) {
    controller.bootstrapStateRequestId = '';
    setControllerLifecycle(
      controller,
      { starting: false, syncing: false },
      'bridge_event_failed',
    );
  }
  if (controller.runStateRequestId === requestId) {
    controller.runStateRequestId = '';
  }
  if (controller.startMessagesRequestId === requestId) {
    controller.startMessagesRequestId = '';
    setControllerLifecycle(
      controller,
      { starting: false, syncing: false },
      'bridge_event_failed',
    );
  }
  if (controller.commandPromptRequestId === requestId) {
    controller.commandPromptRequestId = '';
  }
  if (controller.commandSyncRequestId === requestId) {
    controller.commandSyncRequestId = '';
  }
  if (controller.replacementProbeRequestId === requestId) {
    controller.replacementProbeRequestId = '';
  }
  if (controller.materializationBarrierRequestId === requestId) {
    controller.materializationBarrierRequestId = '';
  }
  if (controller.abortProbeRequestId === requestId) {
    controller.abortProbeRequestId = '';
  }
  if (controller.historyRequestId === requestId) {
    controller.historyRequestId = '';
    setHistoryLoading(controller, false);
  }
  if (controller.pendingSettingRequestId === requestId) {
    controller.pendingSettingRequestId = '';
    controller.pendingEffort = '';
    clearSettingRequestWatch(controller);
  }

  const pending = snapshot.pendingPrompt;
  const rejectsPendingPrompt = Boolean(
    pending &&
    controller.pendingPrompt === pending &&
    (pending.stateRequestId === requestId ||
      pending.messagesRequestId === requestId ||
      pending.settingsRequestId === requestId ||
      (method === 'get_available_thinking_levels' &&
        pending.messagesRequestId)),
  );
  if (rejectsPendingPrompt) {
    cancelPendingPrompt(controller, rpcFailureCopy(method));
  }

  const submitted = snapshot.submittedPrompt;
  if (controller.submittedPrompt === submitted && submitted) {
    if (submitted.admissionStateRequestId === requestId) {
      submitted.admissionStateRequestId = '';
    }
    if (submitted.admissionMessagesRequestId === requestId) {
      submitted.admissionMessagesRequestId = '';
    }
  }

  const rejectsMaterializationState =
    method === 'get_state' &&
    snapshot.materializationStateRequestId === requestId &&
    controller.materializationStateRequestId === requestId;
  const rejectsMaterializationMessages =
    method === 'get_messages' &&
    snapshot.materializationMessagesRequestId === requestId &&
    controller.materializationMessagesRequestId === requestId;
  const rejectsMaterializationPrerequisite =
    method === 'get_available_thinking_levels' &&
    Boolean(snapshot.materializationMessagesRequestId) &&
    controller.materializationMessagesRequestId ===
      snapshot.materializationMessagesRequestId;
  if (rejectsMaterializationState) {
    controller.materializationStateRequestId = '';
  }
  if (rejectsMaterializationMessages || rejectsMaterializationPrerequisite) {
    controller.materializationMessagesRequestId = '';
  }
  if (
    rejectsMaterializationState ||
    rejectsMaterializationMessages ||
    rejectsMaterializationPrerequisite
  ) {
    setControllerLifecycle(
      controller,
      { syncing: false },
      'bridge_event_failed',
    );
    scheduleMaterializationVerificationRetry(controller);
  }
}

async function rpc(
  controller: SessionController,
  request: Record<string, unknown>,
  parentContext?: TraceContext,
): Promise<void> {
  const requestId = stringValue(request.id);
  const method = stringValue(request.type);
  const snapshot = captureRpcDispatchSnapshot(controller);
  const key = requestId
    ? rpcSpanKey(controller.runtimeId, controller.generation, requestId)
    : undefined;
  const expectsResponse = method !== 'extension_ui_response';
  let fireAndForgetSpanEnd: ((outcome: PiRpcOutcome) => void) | undefined;
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
    if (expectsResponse) {
      registerPendingRpcSpan(key, span.end, snapshot, method, span.context);
    } else {
      // Pi consumes extension UI responses without emitting a response envelope.
      fireAndForgetSpanEnd = span.end;
    }
  }
  try {
    await invoke('send_pi', { runtimeId: controller.runtimeId, request });
    fireAndForgetSpanEnd?.('success');
  } catch (error) {
    if (fireAndForgetSpanEnd) fireAndForgetSpanEnd('error');
    else if (key) endPendingRpcSpan(key, 'error');
    if (requestId) {
      cleanupRejectedRpcDispatch(controller, requestId, method, snapshot);
    }
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
    const noticeType = extensionNotifyType(request.notifyType);
    controller.messages.push({
      id,
      kind: 'notice',
      text: request.message,
      ...anchorFields(controller),
      noticeType,
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
  if (method === 'select') return 'Choose an Option';
  if (method === 'confirm') return 'Confirm';
  if (method === 'input') return 'Enter a Value';
  return 'Edit Text';
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
      'New Session',
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
const remoteConnectionTimeoutMessage =
  'The remote connection timed out. Check the connection and try again.';
const remoteConnectionTimeoutMs = 10_000;
const settingRequestTimeoutMs = 10_000;
/** Pi normally emits `agent_start` immediately after prompt preflight succeeds.
 * Wait for that direct confirmation before falling back to state hydration. */
const promptAdmissionReconcileDelay = 150;
const remoteConnectionTimers = new Map<string, ReturnType<typeof setTimeout>>();
const settingRequestTimers = new Map<string, ReturnType<typeof setTimeout>>();

function watchRemoteConnection(controller: SessionController): void {
  clearRemoteConnectionWatch(controller);
  remoteConnectionTimers.set(
    controller.key,
    setTimeout(() => {
      remoteConnectionTimers.delete(controller.key);
      if (controller.disposed || !controller.connectingRemote) return;
      controller.remoteConnectionTimedOut = true;
      if (controller.pendingPrompt) {
        cancelPendingPrompt(controller, remoteConnectionTimeoutMessage);
      }
      presentRemoteConnectionError(controller, remoteConnectionTimeoutMessage);
      void stopControllerProcess(controller, undefined, false);
    }, remoteConnectionTimeoutMs),
  );
}

function clearRemoteConnectionWatch(controller: SessionController): void {
  const timer = remoteConnectionTimers.get(controller.key);
  if (!timer) return;
  remoteConnectionTimers.delete(controller.key);
  clearTimeout(timer);
}

function watchSettingRequest(controller: SessionController): void {
  clearSettingRequestWatch(controller);
  settingRequestTimers.set(
    controller.key,
    setTimeout(() => {
      settingRequestTimers.delete(controller.key);
      if (!controller.pendingSettingRequestId) return;
      controller.pendingSettingRequestId = '';
      controller.pendingEffort = '';
      setControllerError(controller, errorCopy.settingConfirmation);
    }, settingRequestTimeoutMs),
  );
}

function clearSettingRequestWatch(controller: SessionController): void {
  const timer = settingRequestTimers.get(controller.key);
  if (!timer) return;
  settingRequestTimers.delete(controller.key);
  clearTimeout(timer);
}

async function handleBridgeEvent(event: PiBridgeEvent): Promise<void> {
  const controller = controllerByRuntimeId(event.runtimeId);
  if (!controller) return;
  touchController(controller);
  if (event.kind === 'started') {
    if (event.generation > controller.generation) {
      if (controller.submittedPrompt?.generation === controller.generation) {
        const accepted = controller.submittedPrompt.accepted;
        settleInterruptedSubmittedPrompt(controller);
        if (!accepted) {
          setControllerLifecycle(
            controller,
            { working: false },
            'message_send_failed',
          );
        }
      }
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
      clearRemoteConnectionWatch(controller);
      if (controller.pendingPrompt) cancelPendingPrompt(controller, message);
      presentRemoteConnectionError(controller, message);
      return;
    }
    controller.pendingEffort = '';
    controller.pendingSettingRequestId = '';
    clearSettingRequestWatch(controller);
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message);
    } else if (controller.submittedPrompt) {
      const accepted = controller.submittedPrompt.accepted;
      settleInterruptedSubmittedPrompt(controller);
      if (!accepted) {
        setControllerLifecycle(
          controller,
          { working: false },
          'message_send_failed',
        );
      }
      setControllerError(controller, message);
    } else {
      setControllerError(controller, message);
    }
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
    clearMaterializationVerificationWatch(controller);
    clearAbortWatch(controller);
    clearRemoteConnectionWatch(controller);
    discardControllerDialogs(controller, event.generation);
    const remoteConnectionFailed =
      controller.connectingRemote ||
      state.remoteRetry?.controllerKey === controller.key;
    const message =
      event.code === 0
        ? ''
        : remoteConnectionFailed
          ? remotePiProcessExitMessage
          : piProcessExitMessage;
    if (remoteConnectionFailed) {
      const failure =
        message || 'The remote Pi process stopped before it was ready.';
      if (controller.pendingPrompt) cancelPendingPrompt(controller, failure);
      presentRemoteConnectionError(controller, failure);
      return;
    }
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message || 'The Pi process stopped.');
    }
    settleInterruptedSubmittedPrompt(controller);
    controller.compacting = false;
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
    controller.historyRequestId = '';
    setHistoryLoading(controller, false);
    controller.pendingEffort = '';
    controller.pendingSettingRequestId = '';
    controller.remoteConnectionTimedOut = false;
    clearSettingRequestWatch(controller);
    controller.generation = 0;
    controller.status = message;
    await discardReleasedEmptySession(controller);
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
    confirmSubmittedPrompt(controller);
    // Pi can announce B's run before its state response reveals that A was
    // replaced. Keep A's completed-message barrier and settled evidence until
    // that identity read resolves.
    controller.materializationStateRequestId = '';
    controller.materializationMessagesRequestId = '';
    clearSessionReplacementWatch(controller);
    clearMaterializationVerificationWatch(controller);
    clearAbortWatch(controller);
    resetStreamAggregate(controller.runtimeId, controller.generation);
    controller.localErrors = [];
    controller.compacting = false;
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
    const role = stringValue(message?.role);
    if (role === 'user' || role === 'assistant') {
      confirmSubmittedPrompt(controller);
      markPiTranscript(controller);
    }
    if (role !== 'user') return;
    const text = contentText(message?.content);
    const skill = parseSkillBlock(text);
    if (!skill) {
      if (!text) return;
      projectOrdinaryUserMessage(
        controller.messages,
        text,
        `stream-user-${controller.streamSequence++}`,
      );
      return;
    }

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
      delete optimistic.pending;
      if (skill.userMessage) optimistic.skillPrompt = skill.userMessage;
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
    markPiTranscript(controller);
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
  // Pi emits assistant message_end immediately before synchronously appending
  // that finalized message, which flushes a new persistent session. The next
  // identity-scoped RPC response is therefore an ordering barrier behind the
  // append; partial output and command-only runs never cross this boundary.
  if (type === 'message_end') {
    const failure = messageFailure(event.message);
    if (failure) pushError(controller, failure.message, failure.label);
    const message = asRecord(event.message);
    const ephemeral = ephemeralSessionByController(controller.key);
    if (
      stringValue(message?.role) === 'assistant' &&
      ephemeral &&
      !ephemeral.phantom &&
      !workspaceContainsSession(controller) &&
      !controller.materializationVerified &&
      !controller.materializationBarrierRequestId
    ) {
      const requestId = nextRequestId('materialization-barrier');
      controller.materializationBarrierRequestId = requestId;
      await rpc(controller, { id: requestId, type: 'get_state' });
    }
    return;
  }
  if (type === 'compaction_start') {
    controller.compacting = true;
    return;
  }
  /**
   * A failed compaction is reported once and kept nowhere: Pi's message list
   * has no entry for it, so the row is held on the controller and re-appended
   * to every list hydrated until the next run begins.
   */
  if (type === 'compaction_end') {
    controller.compacting = false;
    if (stringValue(event.errorMessage)) {
      const error = {
        key: controller.streamSequence++,
        label: 'Conversation Not Shortened',
        text: 'The conversation could not be shortened. Start a new session or choose a model with a larger context window.',
        ...anchorFields(controller),
      };
      controller.localErrors.push(error);
      // The row shown now and the one merged back later are the same row: it
      // carries the failure's own id and spot rather than passing for a Pi row,
      // which would both stall id adoption and count towards later anchors.
      controller.messages.push({
        id: localErrorId(error.key),
        kind: 'error',
        text: error.text,
        errorLabel: error.label,
        ...(error.anchor === undefined ? {} : { anchor: error.anchor }),
      });
      if (!isControllerSelected(controller)) controller.unread = true;
    } else if (asRecord(event.result)) {
      // A new compaction redraws every history boundary. Keep the visible rows
      // until the settled hydration arrives, but discard the old projection.
      resetHistory(controller);
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
    clearMaterializationVerificationWatch(controller);
    controller.postSettlementHydration = true;
    controller.settledAssistantActivity =
      hasMeaningfulAssistantActivity(controller);
    clearAbortWatch(controller);
    controller.compacting = false;
    setControllerLifecycle(
      controller,
      { syncing: true, working: false, streaming: false, stopping: false },
      'agent_settled',
    );
    controller.status = '';
    if (!isControllerSelected(controller)) controller.unread = true;
    watchSessionReplacement(controller);
    const stateRequestId = nextRequestId('settled-state');
    controller.materializationStateRequestId = stateRequestId;
    if (controller.submittedPrompt?.accepted) {
      controller.submittedPrompt.admissionStateRequestId = stateRequestId;
    }
    await rpc(controller, {
      id: stateRequestId,
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
    controller.status = errorCopy.extensionFailure;
  }
}

function resetHistory(controller: SessionController): void {
  controller.historyLayers = [];
  controller.firstVisibleHistoryLayer = 0;
  controller.historyPrefixLength = 0;
  controller.historyRequestId = '';
}

function setHistoryLoading(
  controller: SessionController,
  loading: boolean,
): void {
  const marker = controller.messages.find(
    (entry) => entry.kind === 'compaction' && entry.historyAvailable,
  );
  if (marker) marker.historyLoading = loading;
}

/** Replaces only the compacted tail; loaded raw history remains above it. */
function applyHistoryTail(
  controller: SessionController,
  tail: ReturnType<typeof hydrateTranscript>,
): void {
  if (controller.historyLayers.length === 0) {
    if (controller.historyRequestId) {
      const marker = tail.find((entry) => entry.kind === 'compaction');
      if (marker) marker.historyLoading = true;
    }
    controller.historyPrefixLength = 0;
    controller.messages = tail;
    return;
  }

  const latestBoundary = tail.find((entry) => entry.kind === 'compaction');
  if (latestBoundary) {
    latestBoundary.historyAvailable = false;
    latestBoundary.historyLoading = false;
  }
  const prefix = historyPrefix(
    controller.historyLayers,
    controller.firstVisibleHistoryLayer,
  );
  controller.historyPrefixLength = prefix.length;
  controller.messages = [...prefix, ...tail];
}

function revealCachedHistory(controller: SessionController): void {
  if (controller.historyLayers.length === 0) return;
  controller.firstVisibleHistoryLayer = Math.max(
    0,
    controller.firstVisibleHistoryLayer - 1,
  );
  const tail = controller.messages.slice(controller.historyPrefixLength);
  applyHistoryTail(controller, tail);
}

async function requestEarlierHistory(
  controller: SessionController,
): Promise<void> {
  if (controller.disposed || !controller.generation) return;
  if (controller.historyRequestId) return;
  if (controller.historyLayers.length > 0) {
    revealCachedHistory(controller);
    return;
  }

  const requestId = nextRequestId('history');
  controller.historyRequestId = requestId;
  setHistoryLoading(controller, true);
  try {
    await rpc(controller, { id: requestId, type: 'get_entries' });
  } catch {
    controller.historyRequestId = '';
    setHistoryLoading(controller, false);
    controller.status = errorCopy.historyLoad;
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
  const responseDispatchStillCurrent = Boolean(
    pendingResult.dispatchSnapshot &&
    pendingResult.method === command &&
    rpcDispatchStillCurrent(controller, pendingResult.dispatchSnapshot),
  );
  if (responseId && confirmedAdmissionRequestIds.delete(responseId)) return;
  if (controller.remoteConnectionTimedOut) return;
  const identityScopedResponse =
    command === 'get_state' ||
    command === 'get_messages' ||
    command === 'get_available_thinking_levels';
  // These reads describe whichever Pi identity owned the runtime when they
  // were dispatched. A replacement keeps the generation but abandons that
  // identity's requests, so only an exact, still-current dispatch may mutate
  // the controller. The response's identity is intentionally not compared:
  // the current identity's own get_state is how replacements are discovered.
  if (identityScopedResponse && !responseDispatchStillCurrent) return;
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
  const resolvesPendingSetting =
    Boolean(controller.pendingSettingRequestId) &&
    responseId === controller.pendingSettingRequestId;
  const resolvesHistory =
    command === 'get_entries' &&
    Boolean(controller.historyRequestId) &&
    responseId === controller.historyRequestId;
  const resolvesSubmittedPrompt =
    command === 'prompt' &&
    controller.submittedPrompt?.requestId === responseId;
  const resolvesAdmissionState =
    command === 'get_state' &&
    Boolean(controller.submittedPrompt?.admissionStateRequestId) &&
    controller.submittedPrompt?.admissionStateRequestId === responseId;
  const resolvesAdmissionMessages =
    command === 'get_messages' &&
    Boolean(controller.submittedPrompt?.admissionMessagesRequestId) &&
    controller.submittedPrompt?.admissionMessagesRequestId === responseId;
  const resolvesMaterializationBarrier =
    command === 'get_state' &&
    Boolean(controller.materializationBarrierRequestId) &&
    controller.materializationBarrierRequestId === responseId;
  const resolvesMaterializationState =
    command === 'get_state' &&
    Boolean(controller.materializationStateRequestId) &&
    controller.materializationStateRequestId === responseId;
  const resolvesMaterializationMessages =
    command === 'get_messages' &&
    Boolean(controller.materializationMessagesRequestId) &&
    controller.materializationMessagesRequestId === responseId;
  const resolvesBootstrap =
    command === 'get_state' &&
    Boolean(controller.bootstrapStateRequestId) &&
    controller.bootstrapStateRequestId === responseId;
  const resolvesPendingState =
    command === 'get_state' &&
    Boolean(controller.pendingPrompt?.stateRequestId) &&
    controller.pendingPrompt?.stateRequestId === responseId;
  const resolvesPendingMessages =
    command === 'get_messages' &&
    Boolean(controller.pendingPrompt?.messagesRequestId) &&
    controller.pendingPrompt?.messagesRequestId === responseId;
  const resolvesMaterializationPrerequisite = Boolean(
    command === 'get_available_thinking_levels' &&
    responseDispatchStillCurrent &&
    pendingResult.dispatchSnapshot?.materializationMessagesRequestId &&
    pendingResult.dispatchSnapshot.materializationMessagesRequestId ===
      controller.materializationMessagesRequestId,
  );
  const resolvesPendingPrerequisite = Boolean(
    command === 'get_available_thinking_levels' &&
    responseDispatchStillCurrent &&
    pendingResult.dispatchSnapshot?.pendingPrompt &&
    pendingResult.dispatchSnapshot.pendingPrompt === controller.pendingPrompt &&
    controller.pendingPrompt?.messagesRequestId,
  );
  if (
    command === 'prompt' &&
    !resolvesSubmittedPrompt &&
    !pendingResult.matched
  ) {
    return;
  }
  if (
    command === 'prompt' &&
    Boolean(controller.commandPromptRequestId) &&
    responseId === controller.commandPromptRequestId
  ) {
    controller.commandPromptRequestId = '';
    if (response.success === true) {
      if (resolvesSubmittedPrompt && controller.submittedPrompt) {
        controller.submittedPrompt.accepted = true;
        controller.submittedPrompt = undefined;
        controller.promptSubmitting = false;
      }
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
    const resolvesCurrentStateRequest =
      command !== 'get_state' ||
      responseDispatchStillCurrent ||
      resolvesBootstrap ||
      resolvesRunState ||
      resolvesReplacementProbe ||
      resolvesCommandSync ||
      resolvesAbortProbe ||
      resolvesAdmissionState ||
      resolvesMaterializationBarrier ||
      resolvesMaterializationState ||
      resolvesPendingState ||
      (command === 'get_state' && resolvesPendingSetting);
    const resolvesCurrentMessagesRequest =
      command !== 'get_messages' ||
      responseDispatchStillCurrent ||
      (Boolean(controller.startMessagesRequestId) &&
        responseId === controller.startMessagesRequestId) ||
      resolvesAdmissionMessages ||
      resolvesMaterializationMessages ||
      resolvesPendingMessages;
    const resolvesCurrentEffortRequest =
      command !== 'get_available_thinking_levels' ||
      responseDispatchStillCurrent;
    if (
      !resolvesCurrentStateRequest ||
      !resolvesCurrentMessagesRequest ||
      !resolvesCurrentEffortRequest
    ) {
      return;
    }

    controller.status = resolvesHistory
      ? errorCopy.historyLoad
      : rpcFailureCopy(command);
    if (resolvesMaterializationBarrier) {
      controller.materializationBarrierRequestId = '';
    }
    if (resolvesMaterializationState) {
      controller.materializationStateRequestId = '';
      controller.materializationMessagesRequestId = '';
      scheduleMaterializationVerificationRetry(controller);
    }
    if (
      resolvesMaterializationMessages ||
      resolvesMaterializationPrerequisite
    ) {
      controller.materializationMessagesRequestId = '';
      scheduleMaterializationVerificationRetry(controller);
    }
    if (resolvesHistory) {
      controller.historyRequestId = '';
      setHistoryLoading(controller, false);
    }
    if (resolvesPendingSetting) {
      controller.pendingSettingRequestId = '';
      controller.pendingEffort = '';
      clearSettingRequestWatch(controller);
    }
    if (resolvesAdmissionState || resolvesAdmissionMessages) {
      releaseSubmittedPrompt(controller);
      setControllerLifecycle(
        controller,
        { working: controller.streaming },
        'prompt_failed',
        responseContext,
      );
      return;
    }
    const pending = controller.pendingPrompt;
    if (resolvesMaterializationPrerequisite) {
      setControllerLifecycle(
        controller,
        { syncing: false },
        'bridge_event_failed',
        responseContext,
      );
    }
    if (resolvesPendingPrerequisite) {
      cancelPendingPrompt(controller, controller.status);
      return;
    }
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
      (responseId === controller.startMessagesRequestId ||
        resolvesMaterializationMessages)
    ) {
      const resolvesStart = responseId === controller.startMessagesRequestId;
      if (resolvesStart) controller.startMessagesRequestId = '';
      setControllerLifecycle(
        controller,
        resolvesStart
          ? { starting: false, syncing: false }
          : { syncing: false },
        'get_messages_failed',
        responseContext,
      );
    }
    if (command === 'prompt') {
      const accepted =
        resolvesSubmittedPrompt &&
        controller.submittedPrompt?.accepted === true;
      if (resolvesSubmittedPrompt) {
        controller.promptSubmitting = false;
        if (accepted) controller.submittedPrompt = undefined;
        else recoverSubmittedPrompt(controller);
      }
      setControllerLifecycle(
        controller,
        { working: accepted ? controller.streaming : false },
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
      try {
        await rpc(controller, {
          id: nextRequestId('session-name-state'),
          type: 'get_state',
        });
      } catch {
        // The confirmed title is already restored; resync is cosmetic here.
      }
    }
    return;
  }
  if (command === 'abort' && responseDispatchStillCurrent) {
    await reconcileAcknowledgedAbort(controller);
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
    if (resolvesMaterializationState) {
      controller.materializationStateRequestId = '';
    }
    const model = asRecord(data.model);
    controller.currentModelProvider = stringValue(model?.provider);
    controller.currentModelId = stringValue(model?.id);
    controller.currentModelName = stringValue(model?.name);
    controller.currentEffort = normalizeEffort(data.thinkingLevel);
    if (resolvesPendingSetting) {
      controller.pendingSettingRequestId = '';
      clearSettingRequestWatch(controller);
    }
    const piSessionId = stringValue(data.sessionId);
    const piSessionPath = stringValue(data.sessionFile);
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.stateRequestId) &&
      responseId === pending?.stateRequestId;
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
    // A persistence barrier or settled-run read can first observe B after Pi
    // moved on, so preserve completed A before B can rebind its row.
    const outgoingIdentity = sessionChanged
      ? captureConnectedSessionIdentity(controller)
      : undefined;
    if (
      outgoingIdentity &&
      (resolvesMaterializationBarrier ||
        Boolean(controller.materializationBarrierRequestId) ||
        shouldRegisterMaterializedPredecessor(controller))
    ) {
      const registered = await registerMaterializedPredecessor(
        controller,
        outgoingIdentity,
      );
      if (!registered) return;
      if (!controllerIdentityMatches(controller, outgoingIdentity)) return;
    }
    const reportedSessionName = stringValue(data.sessionName);
    if (!sessionChanged) applySessionName(controller, reportedSessionName);
    const nowStreaming = data.isStreaming === true;
    if (resolvesAdmissionState && controller.submittedPrompt) {
      controller.submittedPrompt.admissionStateRequestId = '';
    }
    if (data.isCompacting === true) controller.compacting = true;
    else if (data.isCompacting === false || !nowStreaming) {
      controller.compacting = false;
    }
    controller.status = '';
    clearRemoteConnectionWatch(controller);
    finishRemoteConnection(controller);

    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom && !unsavedSession) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }
    if (sessionChanged) applySessionName(controller, reportedSessionName);

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
      clearMaterializationVerificationWatch(controller);
      clearAbortWatch(controller);
      controller.hasPiTranscript = false;
      controller.materializationVerified = false;
      controller.settledAssistantActivity = false;
      controller.materializationBarrierRequestId = '';
      controller.materializationStateRequestId = '';
      controller.materializationMessagesRequestId = '';
      controller.lastUserMessageAt = 0;
      controller.compacting = data.isCompacting === true;
      rebindEphemeralSession(controller);
      controller.messages = [];
      resetHistory(controller);
      controller.models = [];
      controller.efforts = [];
      controller.commands = [];
      controller.commandsLoaded = false;
      controller.pendingEffort = '';
      controller.pendingSettingRequestId = '';
      clearSettingRequestWatch(controller);
      syncingAfterSessionChange = true;
    }
    const materializationBarrierForCurrentSession =
      resolvesMaterializationBarrier && !sessionChanged && !unsavedSession;
    if (materializationBarrierForCurrentSession) {
      clearMaterializationVerificationWatch(controller);
      controller.materializationVerified = true;
    }
    setControllerLifecycle(
      controller,
      {
        ready: true,
        streaming: materializationBarrierForCurrentSession
          ? controller.streaming || nowStreaming
          : nowStreaming,
        stopping: false,
        working: materializationBarrierForCurrentSession
          ? controller.working || controller.streaming || nowStreaming
          : nowStreaming ||
            Boolean(pending) ||
            Boolean(controller.submittedPrompt?.optimisticId),
        connectingRemote: false,
        ...(syncingAfterSessionChange ? { syncing: true } : {}),
      },
      'get_state_response',
      responseContext,
    );

    if (unsavedSession) await retireUnsavedSession(controller);
    if (controller.disposed) return;

    const synchronizedIdentity = captureConnectedSessionIdentity(controller);
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
        sessionChanged ||
          Boolean(resolvesPending) ||
          resolvesCommandSync ||
          resolvesMaterializationBarrier,
      );
    }
    if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
    if (materializationBarrierForCurrentSession) {
      controller.materializationBarrierRequestId = '';
      return;
    }

    if (resolvesPending && pending) {
      controller.bootstrapStateRequestId = '';
      await applyPendingSessionSettings(controller);
      return;
    }
    if (resolvesRunState && !sessionChanged) {
      controller.postSettlementHydration = false;
      controller.settledAssistantActivity = false;
      return;
    }
    if (resolvesAdmissionState && nowStreaming && !sessionChanged) return;
    if (
      resolvesReplacementProbe &&
      !sessionChanged &&
      !controller.postSettlementHydration
    ) {
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
      if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
      await rpc(controller, {
        id: nextRequestId('replacement-commands'),
        type: 'get_commands',
      });
      if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
    }

    const messagesRequestId = nextRequestId('messages');
    if (controller.postSettlementHydration && !nowStreaming) {
      controller.materializationMessagesRequestId = messagesRequestId;
    }
    if (resolvesAdmissionState && controller.submittedPrompt) {
      controller.submittedPrompt.admissionMessagesRequestId = messagesRequestId;
    }
    if (resolvesBootstrap) {
      controller.startMessagesRequestId = messagesRequestId;
      controller.bootstrapStateRequestId = '';
    }
    await rpc(controller, {
      id: nextRequestId('efforts'),
      type: 'get_available_thinking_levels',
    });
    if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
    await rpc(controller, { id: messagesRequestId, type: 'get_messages' });
    return;
  }

  if (command === 'get_messages' && data) {
    const piMessages = Array.isArray(data.messages) ? data.messages : [];
    const gainedTranscript = piMessages.some((value) => {
      const role = stringValue(asRecord(value)?.role);
      return role === 'user' || role === 'assistant';
    });
    const completedAssistant = piMessages.some(
      (value) => stringValue(asRecord(value)?.role) === 'assistant',
    );
    if (gainedTranscript) controller.hasPiTranscript = true;
    const verifiesMaterialization = resolvesMaterializationMessages;
    if (verifiesMaterialization) {
      controller.materializationMessagesRequestId = '';
      if (completedAssistant) {
        clearMaterializationVerificationWatch(controller);
        controller.materializationVerified = true;
      } else {
        scheduleMaterializationVerificationRetry(controller);
      }
    }
    const previous = controller.messages;
    const previousTail = previous.slice(controller.historyPrefixLength);
    controller.messagesLoaded = true;
    const tail = mergeLocalEntries(
      hydrateTranscript(piMessages, previousTail, controller.streaming),
      controller.localErrors,
      previousTail,
    );
    applyHistoryTail(controller, tail);
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.messagesRequestId) &&
      responseId === pending?.messagesRequestId;
    const resolvesStart =
      !resolvesPending && responseId === controller.startMessagesRequestId;
    const resolvesAdmission =
      Boolean(controller.submittedPrompt?.admissionMessagesRequestId) &&
      responseId === controller.submittedPrompt?.admissionMessagesRequestId;
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
    if (resolvesAdmission && controller.submittedPrompt) {
      const submitted = controller.submittedPrompt;
      const confirmed = controller.messages.some(
        (entry) => entry.id === submitted.optimisticId && !entry.pending,
      );
      if (confirmed) confirmSubmittedPrompt(controller);
      else if (!controller.streaming) {
        releaseSubmittedPrompt(controller);
        setControllerLifecycle(
          controller,
          { working: false },
          'prompt_response',
          responseContext,
        );
      } else submitted.admissionMessagesRequestId = '';
    }
    if (verifiesMaterialization && completedAssistant) {
      const materializedIdentity = captureConnectedSessionIdentity(controller);
      await promoteMaterializedSession(controller);
      if (controllerIdentityMatches(controller, materializedIdentity)) {
        controller.postSettlementHydration = false;
        controller.settledAssistantActivity = false;
      }
    }
    // A hidden session that finishes hydrating has nothing left to wait for,
    // so this is where a runtime the user has moved on from is accounted for.
    releaseIdleRuntimes();
    return;
  }

  if (command === 'get_entries' && data && resolvesHistory) {
    controller.historyRequestId = '';
    const layers = historyLayersFromEntries(
      Array.isArray(data.entries) ? data.entries : [],
      stringValue(data.leafId),
    );
    if (layers.length === 0) {
      const marker = controller.messages.find(
        (entry) => entry.kind === 'compaction' && entry.historyAvailable,
      );
      if (marker) {
        marker.historyAvailable = false;
        marker.historyLoading = false;
      }
      return;
    }
    controller.historyLayers = layers;
    // The first activation reveals exactly one layer. Older layers remain
    // behind the new boundary placed at the top of the transcript.
    controller.firstVisibleHistoryLayer = layers.length - 1;
    const tail = controller.messages.slice(controller.historyPrefixLength);
    applyHistoryTail(controller, tail);
    controller.status = '';
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
    if (resolvesSubmittedPrompt && controller.submittedPrompt) {
      const submitted = controller.submittedPrompt;
      submitted.accepted = true;
      controller.promptSubmitting = false;
      if (submitted.optimisticId) {
        setControllerLifecycle(
          controller,
          { working: true },
          'prompt_response',
          responseContext,
        );
        setTimeout(() => {
          void reconcileSubmittedPrompt(controller, submitted, responseContext);
        }, promptAdmissionReconcileDelay);
        return;
      }
      controller.submittedPrompt = undefined;
    }
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
    const stateRequestId = nextRequestId('model-state');
    if (resolvesPendingSetting) {
      controller.pendingSettingRequestId = stateRequestId;
    }
    try {
      await rpc(controller, {
        id: stateRequestId,
        type: 'get_state',
      });
    } catch {
      if (controller.pendingSettingRequestId === stateRequestId) {
        controller.pendingSettingRequestId = '';
        clearSettingRequestWatch(controller);
      }
      setControllerError(controller, errorCopy.modelChange);
    }
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
    if (resolvesPendingSetting) {
      controller.currentEffort = controller.pendingEffort || 'off';
      controller.pendingEffort = '';
      controller.pendingSettingRequestId = '';
      clearSettingRequestWatch(controller);
    }
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
  controller.postSettlementHydration = false;
  controller.settledAssistantActivity = false;
  controller.materializationBarrierRequestId = '';
  controller.materializationStateRequestId = '';
  controller.materializationMessagesRequestId = '';
  clearMaterializationVerificationWatch(controller);
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
  const requestId = nextRequestId('prompt');
  if (prompt.command) controller.commandPromptRequestId = requestId;
  const submission = {
    requestId,
    generation: controller.generation,
    message: prompt.message,
    draft: prompt.draft,
    accepted: false,
    ...(prompt.command ? {} : { optimisticId: prompt.optimisticId }),
  };
  controller.submittedPrompt = submission;
  clearSessionReplacementWatch(controller);
  clearMaterializationVerificationWatch(controller);
  try {
    await rpc(
      controller,
      { id: requestId, type: 'prompt', message: prompt.message },
      prompt.telemetryContext,
    );
    if (
      controller.submittedPrompt?.requestId !== submission.requestId &&
      !submission.accepted
    ) {
      return;
    }
    controller.promptSubmitting = false;
  } catch {
    if (controller.submittedPrompt?.requestId !== submission.requestId) return;
    controller.promptSubmitting = false;
    if (submission.accepted) {
      controller.submittedPrompt = undefined;
      return;
    }
    recoverSubmittedPrompt(controller);
    setControllerLifecycle(
      controller,
      { working: false },
      'pending_prompt_failed',
      prompt.telemetryContext,
    );
    controller.commandPromptRequestId = '';
    setControllerError(controller, errorCopy.messageSend);
  }
}

async function syncAfterCommand(controller: SessionController): Promise<void> {
  if (controller.disposed || !controller.generation) return;
  const requestId = nextRequestId('command-sync');
  controller.commandSyncRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch {
    controller.commandSyncRequestId = '';
    setControllerError(controller, errorCopy.sessionRefresh);
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

function materializationRetryMatches(
  controller: SessionController,
  retry: MaterializationVerificationRetry,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === retry.generation &&
    controller.sessionId === retry.sessionId &&
    controller.sessionPath === retry.sessionPath
  );
}

function scheduleMaterializationVerificationRetry(
  controller: SessionController,
): void {
  if (
    controller.disposed ||
    !controller.generation ||
    !controller.postSettlementHydration ||
    controller.materializationVerified
  ) {
    clearMaterializationVerificationWatch(controller);
    return;
  }

  let retry = materializationVerificationRetries.get(controller.key);
  if (retry && !materializationRetryMatches(controller, retry)) {
    clearMaterializationVerificationWatch(controller);
    retry = undefined;
  }
  retry ??= {
    generation: controller.generation,
    sessionId: controller.sessionId,
    sessionPath: controller.sessionPath,
    nextAttempt: 0,
  };
  if (retry.timer) return;
  if (retry.nextAttempt >= materializationVerificationRetryDelays.length) {
    materializationVerificationRetries.delete(controller.key);
    controller.postSettlementHydration = false;
    controller.settledAssistantActivity = false;
    setControllerLifecycle(
      controller,
      { syncing: false },
      'bridge_event_failed',
    );
    releaseRuntime(controller);
    releaseIdleRuntimes();
    return;
  }

  const delay = materializationVerificationRetryDelays[retry.nextAttempt];
  retry.timer = setTimeout(() => {
    retry.timer = undefined;
    retry.nextAttempt += 1;
    if (!materializationRetryMatches(controller, retry)) {
      if (materializationVerificationRetries.get(controller.key) === retry) {
        materializationVerificationRetries.delete(controller.key);
      }
      return;
    }
    void verifySessionMaterialization(controller, retry);
  }, delay);
  materializationVerificationRetries.set(controller.key, retry);
}

async function verifySessionMaterialization(
  controller: SessionController,
  retry: MaterializationVerificationRetry,
): Promise<void> {
  if (
    !materializationRetryMatches(controller, retry) ||
    !controller.postSettlementHydration ||
    controller.materializationVerified ||
    controller.starting ||
    controller.streaming ||
    controller.working
  ) {
    clearMaterializationVerificationWatch(controller);
    return;
  }
  setControllerLifecycle(
    controller,
    { syncing: true },
    'materialization_retry',
  );
  const requestId = nextRequestId('materialization-retry');
  controller.materializationStateRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch {
    // Transport cleanup schedules the next bounded attempt for this identity.
  }
}

function clearMaterializationVerificationWatch(
  controller: SessionController,
): void {
  const retry = materializationVerificationRetries.get(controller.key);
  if (!retry) return;
  materializationVerificationRetries.delete(controller.key);
  if (retry.timer) clearTimeout(retry.timer);
}

function watchingMaterializationVerification(
  controller: SessionController,
): boolean {
  return materializationVerificationRetries.has(controller.key);
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
  } catch {
    controller.abortProbeRequestId = '';
    setControllerLifecycle(
      controller,
      { stopping: false },
      'abort_probe_failed',
    );
    setControllerError(controller, errorCopy.stopWork);
  }
}

function clearAbortWatch(controller: SessionController): void {
  const timer = abortProbeTimers.get(controller.key);
  if (!timer) return;
  abortProbeTimers.delete(controller.key);
  clearTimeout(timer);
}

/** Pi only answers abort after its agent is idle. Re-probe in case its settle
 * event was lost, but leave the visible lifecycle to that authoritative read. */
async function reconcileAcknowledgedAbort(
  controller: SessionController,
): Promise<void> {
  if (
    controller.disposed ||
    !controller.generation ||
    (!controller.streaming && !controller.stopping)
  ) {
    return;
  }
  clearAbortWatch(controller);
  controller.postSettlementHydration = true;
  controller.settledAssistantActivity =
    hasMeaningfulAssistantActivity(controller);
  setControllerLifecycle(controller, { syncing: true }, 'abort_acknowledged');
  const stateRequestId = nextRequestId('abort-settled-state');
  controller.materializationStateRequestId = stateRequestId;
  if (controller.submittedPrompt?.accepted) {
    controller.submittedPrompt.admissionStateRequestId = stateRequestId;
  }
  await rpc(controller, { id: stateRequestId, type: 'get_state' });
}

function appendOptimisticPrompt(
  controller: SessionController,
  message: string,
  id: string,
): void {
  const invocation = skillInvocation(controller, message);
  if (!invocation) {
    controller.messages.push({
      id,
      kind: 'user',
      text: message,
      pending: true,
    });
    return;
  }

  controller.messages.push({
    id,
    kind: 'skill',
    text: '',
    pending: true,
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

interface ConnectedSessionIdentity {
  generation: number;
  projectPath: string;
  sessionId: string;
  sessionPath: string;
  sessionName: string | null;
  lastUserMessageAt: number | null;
  requiresMaterialization: boolean;
}

const staleRegistration = Symbol('stale-registration');

function captureConnectedSessionIdentity(
  controller: SessionController,
): ConnectedSessionIdentity {
  const ephemeral = ephemeralSessionByController(controller.key);
  return {
    generation: controller.generation,
    projectPath: controller.projectPath,
    sessionId: controller.sessionId,
    sessionPath: controller.sessionPath,
    sessionName: controller.sessionName || firstUserMessage(controller) || null,
    lastUserMessageAt:
      controller.lastUserMessageAt > 0 ? controller.lastUserMessageAt : null,
    requiresMaterialization: Boolean(
      ephemeral && !workspaceContainsSession(controller),
    ),
  };
}

function controllerIdentityMatches(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === identity.generation &&
    controller.projectPath === identity.projectPath &&
    controller.sessionId === identity.sessionId &&
    controller.sessionPath === identity.sessionPath
  );
}

function connectedSessionIdentityMatches(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): boolean {
  return (
    !controller.phantom &&
    controllerIdentityMatches(controller, identity) &&
    (!identity.requiresMaterialization || controller.materializationVerified)
  );
}

function hasMeaningfulAssistantActivity(
  controller: SessionController,
): boolean {
  return controller.messages.some(
    (message) =>
      (message.kind === 'assistant' ||
        message.kind === 'thinking' ||
        message.kind === 'tool') &&
      Boolean(message.text.trim()),
  );
}

function shouldRegisterMaterializedPredecessor(
  controller: SessionController,
): boolean {
  return (
    !controller.phantom &&
    !workspaceContainsSession(controller) &&
    controller.postSettlementHydration &&
    (controller.settledAssistantActivity ||
      hasMeaningfulAssistantActivity(controller))
  );
}

interface RegistrationMutation {
  identityKey: string;
  token: symbol;
}

const latestSessionRegistrationMutations = new Map<string, symbol>();
const sessionRegistrationFlights = new Map<
  string,
  Promise<WorkspaceSnapshot>
>();

function registrationIdentityKey(identity: ConnectedSessionIdentity): string {
  return [
    identity.generation,
    identity.projectPath,
    identity.sessionId,
    identity.sessionPath,
  ].join('\0');
}

function registrationFlightKey(
  identity: ConnectedSessionIdentity,
  adopted: boolean,
): string {
  return JSON.stringify([
    registrationIdentityKey(identity),
    identity.sessionName,
    identity.lastUserMessageAt,
    adopted,
  ]);
}

function beginRegistrationMutation(
  identity: ConnectedSessionIdentity,
): RegistrationMutation {
  const mutation = {
    identityKey: registrationIdentityKey(identity),
    token: Symbol('registration-mutation'),
  };
  latestSessionRegistrationMutations.set(mutation.identityKey, mutation.token);
  return mutation;
}

function registrationMutationIsCurrent(
  mutation: RegistrationMutation,
): boolean {
  return (
    latestSessionRegistrationMutations.get(mutation.identityKey) ===
    mutation.token
  );
}

function finishRegistrationMutation(mutation: RegistrationMutation): void {
  if (registrationMutationIsCurrent(mutation)) {
    latestSessionRegistrationMutations.delete(mutation.identityKey);
  }
}

function failPredecessorRegistration(controller: SessionController): false {
  setControllerLifecycle(controller, { syncing: false }, 'bridge_event_failed');
  setControllerError(controller, errorCopy.sessionRegistration);
  return false;
}

async function registerMaterializedPredecessor(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): Promise<boolean> {
  const mutation = beginRegistrationMutation(identity);
  try {
    const workspace = await registerSession(identity, true);
    if (
      !controllerIdentityMatches(controller, identity) ||
      !registrationMutationIsCurrent(mutation)
    ) {
      return false;
    }
    state.workspace = workspace;
    if (workspaceContainsSession(controller)) return true;
    return failPredecessorRegistration(controller);
  } catch {
    return failPredecessorRegistration(controller);
  } finally {
    finishRegistrationMutation(mutation);
  }
}

async function registerConnectedSession(
  controller: SessionController,
  parentContext?: TraceContext,
  // Set when Pi handed Tau this identity rather than Tau opening a session it
  // already listed: adopting a session means its row must exist and be
  // reachable, even when Tau had archived it before Pi handed it back.
  adopted = false,
  // Set when the caller holds no materialization proof and is using the
  // registration itself as the probe. Pi's snapshot lists the session only
  // when a session file backs it, so the answer comes back either way.
  probeMaterialization = false,
): Promise<void> {
  if (!probeMaterialization && !canRegisterConnectedSession(controller)) return;
  const identity = captureConnectedSessionIdentity(controller);
  const identityHolds = (): boolean =>
    probeMaterialization
      ? !controller.phantom && controllerIdentityMatches(controller, identity)
      : connectedSessionIdentityMatches(controller, identity);
  if (!identityHolds()) return;
  const mutation = beginRegistrationMutation(identity);
  try {
    const workspace = await registerSession(identity, adopted, parentContext);
    if (!identityHolds() || !registrationMutationIsCurrent(mutation)) {
      return;
    }
    state.workspace = workspace;
    if (workspaceContainsSession(controller)) {
      removeRegisteredEphemeralSession(controller);
    }
    if (!isControllerSelected(controller)) return;
    state.activeSessionId = identity.sessionId;
    state.activeSessionPath = identity.sessionPath;
    const selectedWorkspace = await selectRegisteredSession(
      controller,
      identity,
      mutation,
      parentContext,
    );
    if (!identityHolds() || !registrationMutationIsCurrent(mutation)) {
      return;
    }
    state.workspace = selectedWorkspace;
  } catch (error) {
    if (error === staleRegistration) return;
    setControllerError(controller, errorCopy.sessionRegistration);
  } finally {
    finishRegistrationMutation(mutation);
  }
}

function registerSession(
  identity: ConnectedSessionIdentity,
  adopted: boolean,
  parentContext?: TraceContext,
): Promise<WorkspaceSnapshot> {
  const key = registrationFlightKey(identity, adopted);
  const existing = sessionRegistrationFlights.get(key);
  if (existing) return existing;

  const registration = invokeTraced<WorkspaceSnapshot>(
    'register_session',
    {
      projectPath: identity.projectPath,
      sessionId: identity.sessionId,
      sessionPath: identity.sessionPath,
      sessionName: identity.sessionName,
      lastUserMessageAt: identity.lastUserMessageAt,
      adopted,
    },
    parentContext,
  );
  sessionRegistrationFlights.set(key, registration);
  void registration
    .finally(() => {
      if (sessionRegistrationFlights.get(key) === registration) {
        sessionRegistrationFlights.delete(key);
      }
    })
    .catch(() => undefined);
  return registration;
}

// A session Tau is showing has to be selectable. Tau's registry rejecting it
// means the row and the live runtime disagree, so adopt the session Pi is
// holding and select it once more rather than leaving it unreachable.
async function selectRegisteredSession(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
  mutation: RegistrationMutation,
  parentContext?: TraceContext,
): Promise<WorkspaceSnapshot> {
  const selectionIsCurrent = (): boolean =>
    connectedSessionIdentityMatches(controller, identity) &&
    registrationMutationIsCurrent(mutation);
  const select = (): Promise<WorkspaceSnapshot> => {
    if (!selectionIsCurrent()) {
      throw staleRegistration;
    }
    return invokeTraced<WorkspaceSnapshot>(
      'set_active_session',
      {
        projectPath: identity.projectPath,
        sessionId: identity.sessionId,
      },
      parentContext,
    );
  };
  try {
    const workspace = await select();
    if (!selectionIsCurrent()) throw staleRegistration;
    return workspace;
  } catch (error) {
    if (error === staleRegistration || !selectionIsCurrent()) {
      throw staleRegistration;
    }
    const workspace = await registerSession(identity, true, parentContext);
    if (!selectionIsCurrent()) throw staleRegistration;
    state.workspace = workspace;
    const selectedWorkspace = await select();
    if (!selectionIsCurrent()) throw staleRegistration;
    return selectedWorkspace;
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
  } catch {
    setWorkspaceError(errorCopy.sidebarChange);
  }
}

async function persistProjectSelection(
  projectPath: string,
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<void> {
  try {
    if (!workspaceContainsSession(controller)) {
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
  } catch {
    setControllerError(controller, errorCopy.workspaceSelection);
  }
}

// An unregistered row keeps a session reachable while Tau's registry does
// not, so a replacement has to move it onto the identity Pi handed over
// instead of leaving it pointing at the session that was swapped out.
function rebindEphemeralSession(controller: SessionController): void {
  let session = ephemeralSessionByController(controller.key);
  if (!session && !workspaceContainsSession(controller)) {
    session = createPhantomSession(controller.projectPath, controller.key);
    session.phantom = false;
    state.ephemeralSessions.push(session);
  }
  if (!session) return;
  session.id = controller.sessionId;
  session.path = controller.sessionPath;
  session.phantom = false;
  session.title = controller.sessionName || 'New Session';
  // Every workflow phase is a new session even though it reuses this object.
  // Give it fresh activity and selection instead of inheriting the first
  // phase's ordering or leaving the predecessor highlighted.
  session.lastActive = 'now';
  session.sortAt = Date.now();
  if (isControllerSelected(controller)) {
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
  }
}

function markPiTranscript(controller: SessionController): void {
  controller.hasPiTranscript = true;
}

function canRegisterConnectedSession(controller: SessionController): boolean {
  if (controller.phantom) return false;
  const session = ephemeralSessionByController(controller.key);
  return (
    !session ||
    workspaceContainsSession(controller) ||
    controller.materializationVerified
  );
}

async function promoteMaterializedSession(
  controller: SessionController,
): Promise<void> {
  const session = ephemeralSessionByController(controller.key);
  if (
    !session ||
    workspaceContainsSession(controller) ||
    !controller.materializationVerified
  ) {
    return;
  }
  await registerConnectedSession(controller, undefined, true);
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

function restoreSubmittedDraft(
  controller: SessionController,
  submittedDraft: string,
): void {
  if (controller.draft.trim() === submittedDraft.trim()) return;
  controller.draft = controller.draft
    ? `${submittedDraft}\n\n${controller.draft}`
    : submittedDraft;
}

async function reconcileSubmittedPrompt(
  controller: SessionController,
  submitted: NonNullable<SessionController['submittedPrompt']>,
  responseContext?: TraceContext,
): Promise<void> {
  if (
    controller.disposed ||
    !controller.generation ||
    controller.submittedPrompt !== submitted
  ) {
    return;
  }
  const stateRequestId = nextRequestId('prompt-admission-state');
  submitted.admissionStateRequestId = stateRequestId;
  try {
    await rpc(
      controller,
      { id: stateRequestId, type: 'get_state' },
      responseContext,
    );
  } catch {
    if (controller.submittedPrompt !== submitted) return;
    releaseSubmittedPrompt(controller);
    setControllerLifecycle(
      controller,
      { working: controller.streaming },
      'prompt_failed',
      responseContext,
    );
  }
}

function confirmSubmittedPrompt(controller: SessionController): void {
  const submitted = controller.submittedPrompt;
  if (!submitted) return;
  if (submitted.admissionStateRequestId) {
    confirmedAdmissionRequestIds.add(submitted.admissionStateRequestId);
  }
  if (submitted.admissionMessagesRequestId) {
    confirmedAdmissionRequestIds.add(submitted.admissionMessagesRequestId);
  }
  submitted.accepted = true;
  controller.promptSubmitting = false;
  if (submitted.optimisticId) {
    const optimistic = controller.messages.find(
      (message) => message.id === submitted.optimisticId,
    );
    if (optimistic) {
      delete optimistic.pending;
      if (optimistic.kind === 'user') {
        optimistic.pendingUserEvent = 'optimistic';
      }
    }
  }
  controller.submittedPrompt = undefined;
}

function releaseSubmittedPrompt(controller: SessionController): void {
  controller.promptSubmitting = false;
  controller.submittedPrompt = undefined;
}

function settleInterruptedSubmittedPrompt(controller: SessionController): void {
  const submitted = controller.submittedPrompt;
  if (!submitted) return;
  controller.promptSubmitting = false;
  if (submitted.accepted) {
    releaseSubmittedPrompt(controller);
  } else {
    recoverSubmittedPrompt(controller);
  }
}

function recoverSubmittedPrompt(controller: SessionController): void {
  const submitted = controller.submittedPrompt;
  if (!submitted) return;
  controller.submittedPrompt = undefined;
  restoreSubmittedDraft(controller, submitted.draft);
  if (submitted.optimisticId) {
    controller.messages = controller.messages.filter(
      (message) => message.id !== submitted.optimisticId,
    );
  }
}

function cancelPendingPrompt(
  controller: SessionController,
  message: string,
): void {
  const prompt = controller.pendingPrompt;
  if (!prompt) {
    setControllerError(controller, message);
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
  restoreSubmittedDraft(controller, prompt.draft);
  setControllerError(controller, message);
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

function pushError(
  controller: SessionController,
  text: string,
  label = 'Reply Failed',
): void {
  controller.messages.push({
    id: `stream-error-${controller.streamSequence++}`,
    kind: 'error',
    text,
    errorLabel: label,
  });
  if (!isControllerSelected(controller)) controller.unread = true;
}

function retryStatus(event: Record<string, unknown>): string {
  const attempt = String(event.attempt ?? '');
  const maxAttempts = event.maxAttempts ? `/${String(event.maxAttempts)}` : '';
  const failure = describePiError(stringValue(event.errorMessage));
  return `Retrying (${attempt}${maxAttempts}): ${retryPiErrorMessage(failure.kind)}`;
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
  controller.hasPiTranscript = false;
  controller.materializationVerified = false;
  controller.postSettlementHydration = false;
  controller.settledAssistantActivity = false;
  controller.materializationBarrierRequestId = '';
  controller.materializationStateRequestId = '';
  controller.materializationMessagesRequestId = '';
  controller.messages = [];
  if (isControllerSelected(controller)) {
    state.activeSessionId = session.id;
    state.activeSessionPath = '';
  }
  controller.status = errorCopy.unsavedSession;
}

function removeEmptyActivePhantom(): void {
  const controller = activeController.value;
  const session = controller
    ? ephemeralSessionByController(controller.key)
    : undefined;
  // Command-only custom entries and notifications are not Pi transcript
  // content. Until Pi reports a user or assistant message, this row remains
  // as disposable as the unsent session it came from.
  const unansweredUnsavedCommand = Boolean(
    controller?.commandPromptRequestId && !controller.streaming,
  );
  if (
    !controller ||
    !session ||
    workspaceContainsSession(controller) ||
    controller.lastUserMessageAt > 0 ||
    controller.hasPiTranscript ||
    controller.draft.trim() ||
    (controller.working && !unansweredUnsavedCommand) ||
    controllerHasPendingDialog(controller)
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
      clearMaterializationVerificationWatch(controller);
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
    clearMaterializationVerificationWatch(controller);
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
    !controller.pendingSettingRequestId &&
    !controller.pendingSessionRename &&
    !controllerHasPendingDialog(controller) &&
    !watchingSessionReplacement(controller) &&
    !watchingMaterializationVerification(controller)
  );
}

function sessionHasPiActivity(controller: SessionController): boolean {
  return (
    controller.hasPiTranscript ||
    controller.lastUserMessageAt > 0 ||
    hasMeaningfulAssistantActivity(controller)
  );
}

async function discardReleasedEmptySession(
  controller: SessionController,
): Promise<boolean> {
  const ephemeral = ephemeralSessionByController(controller.key);
  if (
    !ephemeral ||
    workspaceContainsSession(controller) ||
    controller.materializationVerified ||
    controller.draft.trim() ||
    controllerHasPendingDialog(controller)
  ) {
    return false;
  }
  // Tau losing its runtime before it could verify materialization is not
  // evidence that Pi has no file for this session: a run that streamed for an
  // hour without settling never verifies. Register to find out, because the
  // snapshot that comes back lists the session only when a file backs it, so a
  // real session is adopted here and an unwritten one still falls through.
  if (
    controller.sessionId &&
    controller.sessionPath &&
    sessionHasPiActivity(controller)
  ) {
    await registerConnectedSession(controller, undefined, true, true);
    if (controller.disposed || workspaceContainsSession(controller)) {
      return false;
    }
  }
  const released = ephemeralSessionByController(controller.key);
  if (!released) return false;
  removeEphemeralSession(released, true);
  return true;
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
  restartSelected = true,
): Promise<void> {
  if (!controller.generation && !controller.starting) return;
  const generation = controller.generation;
  const stoppedSessionId = controller.sessionId;
  try {
    await invokeTraced(
      'stop_pi',
      { runtimeId: controller.runtimeId },
      parentContext,
    );
  } catch {
    setControllerError(controller, errorCopy.closePi);
    return;
  }
  if (controller.generation !== generation) return;
  // Pi replaces a session on a live runtime without starting a new process, so
  // the generation check above cannot see it. A stop authorised while the
  // predecessor sat idle must not be read as permission to drop the successor
  // that landed while it was in flight.
  const replacedDuringStop = controller.sessionId !== stoppedSessionId;
  clearSessionReplacementWatch(controller);
  clearMaterializationVerificationWatch(controller);
  clearAbortWatch(controller);
  clearRemoteConnectionWatch(controller);
  discardControllerDialogs(controller, generation);
  abandonPendingRpcSpans(controller.runtimeId, generation, 'abandoned_stop');
  flushStreamAggregate(controller.runtimeId, generation);
  controller.generation = 0;
  controller.compacting = false;
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
  controller.historyRequestId = '';
  setHistoryLoading(controller, false);
  controller.pendingEffort = '';
  controller.pendingSettingRequestId = '';
  controller.remoteConnectionTimedOut = false;
  clearSettingRequestWatch(controller);

  if (!replacedDuringStop && (await discardReleasedEmptySession(controller))) {
    return;
  }

  if (
    (replacedDuringStop ||
      (restartSelected && isControllerSelected(controller))) &&
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
  requestEarlierHistory,
  applyPendingSessionSettings,
  applyPendingSessionEffort,
  requestPendingMessages,
  dispatchPendingPrompt,
  syncAfterCommand,
  watchSessionReplacement,
  probeSessionReplacement,
  scheduleMaterializationVerificationRetry,
  watchingMaterializationVerification,
  clearSessionReplacementWatch,
  watchingSessionReplacement,
  watchAbort,
  probeAbort,
  clearAbortWatch,
  watchRemoteConnection,
  clearRemoteConnectionWatch,
  watchSettingRequest,
  clearSettingRequestWatch,
  appendOptimisticPrompt,
  skillInvocation,
  invokesExtensionCommand,
  applySessionName,
  persistSessionName,
  registerConnectedSession,
  persistExpandedProject,
  persistProjectSelection,
  materializePendingSession,
  recoverSubmittedPrompt,
  cancelPendingPrompt,
  pushError,
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
  remoteConnectionTimeoutMessage,
  remoteConnectionTimeoutMs,
  settingRequestTimeoutMs,
};
