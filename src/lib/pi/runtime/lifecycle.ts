import { invoke } from '@tauri-apps/api/core';

import {
  abortAcknowledgeDelay,
  abortProbeTimers,
  activeController,
  clearActiveSession,
  controllerByKey,
  controllerHasPendingDialog,
  createPhantomSession,
  sessionTitleMarkdown,
  ephemeralSessionByController,
  idleRuntimeLimit,
  isControllerSelected,
  nextRequestId,
  presentRemoteConnectionError,
  replacementProbeDelays,
  replacementProbeTimers,
  setControllerError,
  setControllerLifecycle,
  state,
  touchController,
  workspaceContainsSession,
  type EphemeralSession,
  type ProjectSummary,
  type SessionController,
} from '../../../composables/state';
import { errorCopy, rpcFailureCopy } from '../../error-copy';
import { invokeTraced, startRpcSpan } from '../../telemetry';
import { PiRpcMethod, PiRpcOutcome } from '../../telemetry/attributes';
import { TraceContext } from '../../telemetry/trace-context';
import { piOwnerArgs } from '../ownership';
import { queueHasWork } from '../queue';
import {
  rpcSpanKey,
  registerPendingRpcSpan,
  endPendingRpcSpan,
  abandonPendingRpcSpans,
  flushStreamAggregate,
  type RpcDispatchSnapshot,
} from '../rpc-bookkeeping';
import { stringValue } from '../transcript';

import { discardControllerDialogs } from './extensions';
import { setHistoryLoading } from './history';
import {
  settleInterruptedSubmittedPrompt,
  cancelPendingPrompt,
} from './prompts';
import { resetQueue } from './queue-state';
import {
  staleRegistration,
  captureConnectedSessionIdentity,
  controllerIdentityMatches,
  connectedSessionIdentityMatches,
  hasMeaningfulAssistantActivity,
  registrationArchiveSnapshot,
  mergeRegisteredSession,
  beginRegistrationMutation,
  registrationMutationIsCurrent,
  finishRegistrationMutation,
  registerSession,
  selectRegisteredSession,
  canRegisterConnectedSession,
} from './registration';

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

async function startController(
  controller: SessionController,
  project: ProjectSummary,
  sessionPath?: string,
  preserveMessages = false,
  parentContext?: TraceContext,
): Promise<void> {
  if (controller.starting || controller.disposed) return;
  resetQueue(controller, controller.generation > 0);
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
  controller.compactionReconciliationPending = false;
  controller.compactionStreamSequence = controller.streamSequence;
  controller.bootstrapStateRequestId = '';
  controller.bootstrapSessionPath = sessionPath ?? '';
  controller.runStateRequestId = '';
  controller.startMessagesRequestId = '';
  controller.historyRequestId = '';
  setHistoryLoading(controller, false);
  controller.commandPromptRequestId = '';
  controller.commandSyncRequestId = '';
  controller.replacementProbeRequestId = '';
  clearSessionNameRefresh(controller);
  controller.sessionNameRevision = 0;
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
  controller.retry = undefined;
  discardControllerDialogs(controller);
  void refreshModelScope(controller, project, parentContext);

  try {
    if (project.connectionString) {
      controller.generation = await invokeTraced<number>(
        'start_pi_remote',
        {
          ...piOwnerArgs(),
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
          ...piOwnerArgs(),
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
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message, !project.connectionString);
    }
    if (project.connectionString && controller.reconnectingRemote) {
      controller.reconnectingRemote = false;
      controller.remoteDisconnected = true;
      setControllerError(controller, remoteReconnectFailureMessage);
    } else if (project.connectionString) {
      presentRemoteConnectionError(controller, message);
    } else setControllerError(controller, message);
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

function captureRpcDispatchSnapshot(
  controller: SessionController,
): RpcDispatchSnapshot {
  return {
    generation: controller.generation,
    sessionId: controller.sessionId,
    sessionPath: controller.sessionPath,
    messagesHydrationSequence: controller.messagesHydrationSequence,
    sessionNameRevision: controller.sessionNameRevision,
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
  method?: string,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === snapshot.generation &&
    controller.sessionId === snapshot.sessionId &&
    controller.sessionPath === snapshot.sessionPath &&
    (method !== 'get_messages' ||
      controller.messagesHydrationSequence ===
        snapshot.messagesHydrationSequence)
  );
}

function cleanupRejectedRpcDispatch(
  controller: SessionController,
  requestId: string,
  method: string,
  snapshot: RpcDispatchSnapshot,
): void {
  if (!rpcDispatchStillCurrent(controller, snapshot, method)) return;

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
  if (controller.sessionNameStateRequestId === requestId) {
    const revision = controller.sessionNameStateRevision;
    const trailing = finishSessionNameRefresh(controller, requestId, revision);
    if (trailing) void requestSessionNameRefresh(controller);
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
  if (method === 'get_messages') {
    if (controller.startMessagesRequestId) {
      controller.startMessagesRequestId = requestId;
    }
    if (controller.materializationMessagesRequestId) {
      controller.materializationMessagesRequestId = requestId;
    }
    if (controller.pendingPrompt?.messagesRequestId) {
      controller.pendingPrompt.messagesRequestId = requestId;
    }
    if (controller.submittedPrompt?.admissionMessagesRequestId) {
      controller.submittedPrompt.admissionMessagesRequestId = requestId;
    }
    controller.messagesHydrationSequence += 1;
  }
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
    await invoke('send_pi', {
      ...piOwnerArgs(),
      runtimeId: controller.runtimeId,
      request,
    });
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

const piConnectionFailureMessage =
  'The Pi connection failed. Select the session again to reconnect.';

const piProcessExitMessage =
  'The Pi process stopped unexpectedly. Select the session again to reconnect.';

const remotePiConnectionFailureMessage =
  'The remote Pi connection failed. Check the connection and try again.';

const remotePiProcessExitMessage =
  'The remote Pi process stopped unexpectedly. Check the connection and try again.';

const remoteDisconnectedMessage = 'The connection was lost.';

const remoteReconnectFailureMessage =
  'The remote connection failed. Try reconnecting again.';

const remoteConnectionTimeoutMessage =
  'The remote connection timed out. Check the connection and try again.';

const remoteConnectionTimeoutMs = 10_000;

const settingRequestTimeoutMs = 10_000;

const sessionNameRefreshTimeoutMs = 10_000;

const promptAdmissionReconcileDelay = 150;

const remoteConnectionTimers = new Map<string, ReturnType<typeof setTimeout>>();

const settingRequestTimers = new Map<string, ReturnType<typeof setTimeout>>();

const sessionNameRefreshTimers = new Map<
  string,
  ReturnType<typeof setTimeout>
>();

function watchRemoteConnection(controller: SessionController): void {
  clearRemoteConnectionWatch(controller);
  remoteConnectionTimers.set(
    controller.key,
    setTimeout(() => {
      remoteConnectionTimers.delete(controller.key);
      if (controller.disposed || !controller.connectingRemote) return;
      controller.remoteConnectionTimedOut = true;
      if (controller.pendingPrompt) {
        cancelPendingPrompt(controller, remoteConnectionTimeoutMessage, false);
      }
      if (controller.reconnectingRemote) {
        controller.reconnectingRemote = false;
        controller.remoteDisconnected = true;
        setControllerLifecycle(
          controller,
          { starting: false, connectingRemote: false, syncing: false },
          'bridge_event_failed',
        );
        setControllerError(controller, remoteReconnectFailureMessage);
      } else {
        presentRemoteConnectionError(
          controller,
          remoteConnectionTimeoutMessage,
        );
      }
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

function clearSessionNameRefresh(controller: SessionController): void {
  const timer = sessionNameRefreshTimers.get(controller.key);
  if (timer) clearTimeout(timer);
  sessionNameRefreshTimers.delete(controller.key);
  controller.sessionNameStateRequestId = '';
  controller.sessionNameStateRevision = 0;
}

function finishSessionNameRefresh(
  controller: SessionController,
  requestId: string,
  revision: number,
): boolean {
  if (controller.sessionNameStateRequestId !== requestId) return false;
  clearSessionNameRefresh(controller);
  return controller.sessionNameRevision > revision;
}

async function requestSessionNameRefresh(
  controller: SessionController,
): Promise<void> {
  if (
    controller.disposed ||
    !controller.generation ||
    controller.sessionNameStateRequestId
  ) {
    return;
  }
  const requestId = nextRequestId('session-name-state');
  const revision = controller.sessionNameRevision;
  controller.sessionNameStateRequestId = requestId;
  controller.sessionNameStateRevision = revision;
  sessionNameRefreshTimers.set(
    controller.key,
    setTimeout(() => {
      if (controller.sessionNameStateRequestId !== requestId) return;
      endPendingRpcSpan(
        rpcSpanKey(controller.runtimeId, controller.generation, requestId),
        'timeout',
      );
      const trailing = finishSessionNameRefresh(
        controller,
        requestId,
        revision,
      );
      if (trailing) void requestSessionNameRefresh(controller);
    }, sessionNameRefreshTimeoutMs),
  );
  try {
    await rpc(controller, { id: requestId, type: 'get_state' });
  } catch {
    // Transport cleanup clears this cosmetic request. A later notification or
    // ordinary state synchronization can retry without stopping the runtime.
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
    controller.working ||
    controllerHasPendingDialog(controller)
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
  if (timers) {
    replacementProbeTimers.delete(controller.key);
    for (const timer of timers) clearTimeout(timer);
  }
  controller.replacementProbeRequestId = '';
}

function watchingSessionReplacement(controller: SessionController): boolean {
  // The last timer leaves the schedule before its get_state response arrives.
  // Keep idle eviction away from the runtime until that identity read settles.
  return (
    replacementProbeTimers.has(controller.key) ||
    Boolean(controller.replacementProbeRequestId)
  );
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

function applySessionName(
  controller: SessionController,
  name: string,
  fallbackTitle = '',
  fallbackTitleMarkdown?: string,
): void {
  controller.sessionName = name;
  const title = name || fallbackTitle;
  if (!title) return;
  const titleMarkdown = fallbackTitleMarkdown ?? sessionTitleMarkdown(title);

  const ephemeral = ephemeralSessionByController(controller.key);
  if (ephemeral && !ephemeral.phantom) {
    ephemeral.title = title;
    ephemeral.titleMarkdown = titleMarkdown;
    return;
  }

  const project = state.workspace?.projects.find(
    (candidate) => candidate.path === controller.projectPath,
  );
  const session = project?.sessions.find(
    (candidate) => candidate.id === controller.sessionId,
  );
  if (session) {
    session.title = title;
    session.titleMarkdown = titleMarkdown;
  }
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
  // Set when the caller holds no materialization proof and is using the
  // registration itself as the probe. Local Pi returns a row only when a
  // session file backs it, so the answer comes back either way.
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
  const archivedAtDispatch = registrationArchiveSnapshot(identity);
  try {
    const session = await registerSession(identity, adopted, parentContext);
    if (!identityHolds() || !registrationMutationIsCurrent(mutation)) {
      return;
    }
    if (
      !mergeRegisteredSession(identity.projectPath, session, archivedAtDispatch)
    )
      return;
    removeRegisteredEphemeralSession(controller);
    if (
      !isControllerSelected(controller) ||
      (session?.selected && state.activeProjectPath === identity.projectPath)
    )
      return;
    state.activeSessionId = identity.sessionId;
    state.activeSessionPath = identity.sessionPath;
    await selectRegisteredSession(
      controller,
      identity,
      mutation,
      archivedAtDispatch,
      parentContext,
    );
  } catch (error) {
    if (error === staleRegistration) return;
    setControllerError(controller, errorCopy.sessionRegistration);
  } finally {
    finishRegistrationMutation(mutation);
  }
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

async function retireUnsavedSession(
  controller: SessionController,
): Promise<void> {
  const staleSessionId = controller.sessionId;
  if (workspaceContainsSession(controller)) {
    try {
      const result = await invokeTraced<{
        sessionId: string;
        archived: boolean;
        activeSessionId: string;
      }>('archive_session', {
        projectPath: controller.projectPath,
        sessionId: staleSessionId,
      });
      const project = state.workspace?.projects.find(
        (entry) => entry.path === controller.projectPath,
      );
      const row = project?.sessions.find(
        (entry) => entry.id === staleSessionId,
      );
      if (row && result.sessionId === staleSessionId) {
        row.archived = result.archived;
      }
      if (project) {
        for (const entry of project.sessions) {
          entry.selected = entry.id === result.activeSessionId;
        }
      }
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
  controller.compactionReconciliationPending = false;
  controller.compactionStreamSequence = controller.streamSequence;
  controller.messages = [];
  if (isControllerSelected(controller)) {
    state.activeSessionId = session.id;
    state.activeSessionPath = '';
  }
  setControllerError(controller, errorCopy.unsavedSession);
}

function removeEmptyActivePhantom(): void {
  const controller = activeController.value;
  const session = controller
    ? ephemeralSessionByController(controller.key)
    : undefined;
  // Command-only custom entries and notifications are not Pi transcript
  // content. Until Pi reports a user or assistant message, this row remains
  // as disposable as the unsent session it came from. A workflow successor is
  // the exception while Tau verifies the identity an extension just created.
  const unansweredUnsavedCommand = Boolean(
    controller?.commandPromptRequestId && !controller.streaming,
  );
  if (
    !controller ||
    !session ||
    workspaceContainsSession(controller) ||
    controller.lastUserMessageAt > 0 ||
    controller.hasPiTranscript ||
    controller.postSettlementHydration ||
    controller.draft.trim() ||
    queueHasWork(controller.queue, controller.queueSubmissions) ||
    controller.queueFailedDrafts.length > 0 ||
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
      controller.retry = undefined;
      clearSessionReplacementWatch(controller);
      clearSessionNameRefresh(controller);
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
    controller.retry = undefined;
    clearSessionReplacementWatch(controller);
    clearSessionNameRefresh(controller);
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
    !queueHasWork(controller.queue, controller.queueSubmissions) &&
    !controller.queuePreparing &&
    !controller.queueClearing &&
    !controller.pendingSessionRename &&
    !controller.sessionNameStateRequestId &&
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
    controller.remoteDisconnected ||
    controller.draft.trim() ||
    queueHasWork(controller.queue, controller.queueSubmissions) ||
    controller.queueFailedDrafts.length > 0 ||
    controllerHasPendingDialog(controller)
  ) {
    return false;
  }
  // Tau losing its runtime before it could verify materialization is not
  // evidence that Pi has no file for this session: a run that streamed for an
  // hour without settling never verifies. Register to find out, because the
  // returned row exists only when a file backs it, so a real session is
  // adopted here and an unwritten one still falls through.
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
      { ...piOwnerArgs(), runtimeId: controller.runtimeId },
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
  clearSessionNameRefresh(controller);
  clearMaterializationVerificationWatch(controller);
  clearAbortWatch(controller);
  clearRemoteConnectionWatch(controller);
  discardControllerDialogs(controller, generation);
  abandonPendingRpcSpans(controller.runtimeId, generation, 'abandoned_stop');
  flushStreamAggregate(controller.runtimeId, generation);
  resetQueue(controller, true);
  controller.generation = 0;
  controller.retry = undefined;
  controller.compacting = false;
  controller.compactionReconciliationPending = false;
  controller.compactionStreamSequence = controller.streamSequence;
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
  startController,
  refreshModelScope,
  requestBootstrap,
  rpcDispatchStillCurrent,
  rpc,
  piConnectionFailureMessage,
  piProcessExitMessage,
  remotePiConnectionFailureMessage,
  remotePiProcessExitMessage,
  remoteDisconnectedMessage,
  remoteReconnectFailureMessage,
  remoteConnectionTimeoutMessage,
  remoteConnectionTimeoutMs,
  settingRequestTimeoutMs,
  promptAdmissionReconcileDelay,
  watchRemoteConnection,
  clearRemoteConnectionWatch,
  watchSettingRequest,
  clearSettingRequestWatch,
  clearSessionNameRefresh,
  finishSessionNameRefresh,
  requestSessionNameRefresh,
  syncAfterCommand,
  watchSessionReplacement,
  probeSessionReplacement,
  clearSessionReplacementWatch,
  watchingSessionReplacement,
  scheduleMaterializationVerificationRetry,
  clearMaterializationVerificationWatch,
  watchingMaterializationVerification,
  watchAbort,
  probeAbort,
  clearAbortWatch,
  reconcileAcknowledgedAbort,
  applySessionName,
  persistSessionName,
  registerConnectedSession,
  promoteMaterializedSession,
  retireUnsavedSession,
  removeEmptyActivePhantom,
  discardUnregisteredEphemeralSession,
  removeRegisteredEphemeralSession,
  removeEphemeralSession,
  removeProjectUiState,
  canReleaseRuntime,
  discardReleasedEmptySession,
  releaseRuntime,
  releaseIdleRuntimes,
  stopControllerProcess,
};
