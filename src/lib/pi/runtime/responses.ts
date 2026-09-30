import {
  commandOption,
  finishRemoteConnection,
  nextRequestId,
  normalizeEffort,
  resolveRemoteFeedback,
  setControllerError,
  setControllerLifecycle,
  type SessionController,
} from '../../../composables/state';
import { CommandOption } from '../../commands';
import { errorCopy, rpcFailureCopy } from '../../error-copy';
import { recordRpcResponseAnomaly } from '../../telemetry';
import { ModelOption } from '../model-scope';
import {
  rpcSpanKey,
  endPendingRpcSpan,
  abandonPendingRpcSpans,
} from '../rpc-bookkeeping';
import {
  asRecord,
  hydrateTranscript,
  historyLayersFromEntries,
  mergeLocalEntries,
  stringValue,
} from '../transcript';

import {
  applyPendingSessionSettings,
  applyPendingSessionEffort,
  requestPendingMessages,
  dispatchPendingPrompt,
  reconcileSubmittedPrompt,
} from './delivery';
import {
  resetHistory,
  setHistoryLoading,
  applyHistoryTail,
  mergeLiveStreamSuffix,
  reanchorCompactionLocalEntries,
} from './history';
import {
  rpcDispatchStillCurrent,
  rpc,
  remoteReconnectFailureMessage,
  promptAdmissionReconcileDelay,
  clearRemoteConnectionWatch,
  clearSettingRequestWatch,
  clearSessionNameRefresh,
  finishSessionNameRefresh,
  requestSessionNameRefresh,
  syncAfterCommand,
  clearCommandRefreshWatch,
  failCommandRefresh,
  clearSessionReplacementWatch,
  scheduleMaterializationVerificationRetry,
  clearMaterializationVerificationWatch,
  clearAbortWatch,
  reconcileAcknowledgedAbort,
  applySessionName,
  persistSessionName,
  registerConnectedSession,
  promoteMaterializedSession,
  retireUnsavedSession,
  releaseOrCacheRuntime,
  releaseIdleRuntimes,
  stopControllerProcess,
} from './lifecycle';
import {
  confirmedAdmissionRequestIds,
  confirmSubmittedPrompt,
  releaseSubmittedPrompt,
  recoverSubmittedPrompt,
  cancelPendingPrompt,
} from './prompts';
import { queueWaiters, queueIdentityCurrent, resetQueue } from './queue-state';
import {
  captureConnectedSessionIdentity,
  controllerIdentityMatches,
  shouldRegisterMaterializedPredecessor,
  registerMaterializedPredecessor,
  rebindEphemeralSession,
  materializePendingSession,
} from './registration';

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
  const queueWaiter = queueWaiters.get(responseId);
  if (
    queueWaiter &&
    queueWaiter.controller.key === controller.key &&
    queueWaiter.controller.runtimeId === controller.runtimeId &&
    queueWaiter.method === command
  ) {
    queueWaiter.resolve(
      queueIdentityCurrent(
        controller,
        queueWaiter.generation,
        queueWaiter.sessionId,
        queueWaiter.sessionPath,
      )
        ? response.success === true
          ? 'ok'
          : 'rejected'
        : 'uncertain',
    );
    return;
  }
  if (
    command === 'set_steering_mode' ||
    command === 'set_follow_up_mode' ||
    command === 'clear_queue' ||
    (command === 'prompt' && responseId.startsWith('queue-prompt'))
  )
    return;
  const sessionNameRevisionAtResponse = controller.sessionNameRevision;
  const responseDispatchStillCurrent = Boolean(
    pendingResult.dispatchSnapshot &&
    pendingResult.method === command &&
    rpcDispatchStillCurrent(
      controller,
      pendingResult.dispatchSnapshot,
      command,
    ),
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
  const resolvesSessionNameState =
    command === 'get_state' &&
    Boolean(controller.sessionNameStateRequestId) &&
    responseId === controller.sessionNameStateRequestId;
  const sessionNameStateRevision = resolvesSessionNameState
    ? controller.sessionNameStateRevision
    : 0;
  const resolvesCommandSync =
    command === 'get_state' &&
    Boolean(controller.commandSyncRequestId) &&
    responseId === controller.commandSyncRequestId;
  if (resolvesCommandSync && response.success !== true)
    controller.commandSyncRequestId = '';
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
  const resolvesStartMessages =
    command === 'get_messages' &&
    Boolean(controller.startMessagesRequestId) &&
    controller.startMessagesRequestId === responseId;
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
    !(
      pendingResult.matched &&
      pendingResult.method === 'prompt' &&
      pendingResult.dispatchSnapshot?.generation === controller.generation
    )
  ) {
    return;
  }
  if (
    command === 'set_session_name' &&
    controller.pendingSessionRename?.requestId !== responseId
  ) {
    return;
  }
  if (
    command === 'prompt' &&
    Boolean(controller.commandPromptRequestId) &&
    responseId === controller.commandPromptRequestId &&
    pendingResult.matched &&
    pendingResult.method === 'prompt' &&
    pendingResult.dispatchSnapshot?.generation === controller.generation
  ) {
    if (response.success === true) {
      // Establish the read watch before releasing command execution ownership.
      void syncAfterCommand(controller);
      controller.commandPromptRequestId = '';
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
      return;
    }
    controller.commandPromptRequestId = '';
  }
  if (response.success !== true) {
    if (resolvesSessionNameState) {
      const trailing = finishSessionNameRefresh(
        controller,
        responseId,
        sessionNameStateRevision,
      );
      if (trailing) void requestSessionNameRefresh(controller);
      return;
    }
    const resolvesCurrentStateRequest =
      command !== 'get_state' ||
      responseDispatchStillCurrent ||
      resolvesBootstrap ||
      resolvesRunState ||
      resolvesReplacementProbe ||
      resolvesSessionNameState ||
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
      resolvesStartMessages ||
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

    const failureMessage = resolvesHistory
      ? errorCopy.historyLoad
      : rpcFailureCopy(command);
    setControllerError(controller, failureMessage);
    if (
      resolvesCommandSync ||
      (command === 'get_messages' &&
        controller.commandMessagesRequestId === responseId)
    ) {
      failCommandRefresh(controller);
    }
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
    if (
      command === 'get_messages' &&
      (resolvesStartMessages || resolvesMaterializationMessages)
    ) {
      if (resolvesStartMessages) controller.startMessagesRequestId = '';
      setControllerLifecycle(
        controller,
        resolvesStartMessages
          ? { ready: false, starting: false, syncing: false }
          : { syncing: false },
        'get_messages_failed',
        responseContext,
      );
      if (resolvesStartMessages && controller.reconnectingRemote) {
        controller.reconnectingRemote = false;
        controller.remoteDisconnected = true;
        setControllerError(controller, remoteReconnectFailureMessage);
        void stopControllerProcess(controller, responseContext, false);
        return;
      }
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
      cancelPendingPrompt(controller, failureMessage);
      return;
    }
    const failedPendingSetting =
      Boolean(pending?.settingsRequestId) &&
      responseId === pending?.settingsRequestId;
    if (failedPendingSetting) {
      cancelPendingPrompt(controller, failureMessage);
      return;
    }
    const failedPendingRequest =
      Boolean(pending) &&
      ((command === 'get_state' && responseId === pending?.stateRequestId) ||
        (command === 'get_messages' &&
          responseId === pending?.messagesRequestId));
    if (failedPendingRequest) {
      cancelPendingPrompt(controller, failureMessage);
    }
    if (command === 'get_state') {
      if (resolvesBootstrap) controller.bootstrapStateRequestId = '';
      setControllerLifecycle(
        controller,
        resolvesBootstrap
          ? { ready: false, syncing: false, starting: false }
          : { syncing: false },
        'get_state_failed',
        responseContext,
      );
      if (resolvesBootstrap && controller.reconnectingRemote) {
        controller.reconnectingRemote = false;
        controller.remoteDisconnected = true;
        setControllerError(controller, remoteReconnectFailureMessage);
        void stopControllerProcess(controller, responseContext, false);
        return;
      }
      releaseOrCacheRuntime(controller);
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
        controller.sessionNameRevision += 1;
        applySessionName(
          controller,
          pending.previousName,
          pending.previousTitle,
          pending.previousTitleMarkdown,
        );
        controller.pendingSessionRename = undefined;
      }
      await requestSessionNameRefresh(controller);
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
  if (resolvesSessionNameState && !data) {
    const trailing = finishSessionNameRefresh(
      controller,
      responseId,
      sessionNameStateRevision,
    );
    if (trailing) void requestSessionNameRefresh(controller);
    return;
  }

  let trailingSessionNameRefresh: boolean;
  if (command === 'get_state' && data) {
    if (resolvesSessionNameState) {
      const piSessionId = stringValue(data.sessionId);
      const piSessionPath = stringValue(data.sessionFile);
      const sessionChanged =
        !controller.phantom &&
        Boolean(piSessionId && piSessionPath) &&
        (controller.sessionId !== piSessionId ||
          controller.sessionPath !== piSessionPath);
      if (!piSessionId || !piSessionPath || !sessionChanged) {
        const revisionIsCurrent =
          sessionNameStateRevision === controller.sessionNameRevision;
        const trailing = finishSessionNameRefresh(
          controller,
          responseId,
          sessionNameStateRevision,
        );
        if (
          piSessionId &&
          piSessionPath &&
          revisionIsCurrent &&
          !controller.pendingSessionRename
        ) {
          applySessionName(controller, stringValue(data.sessionName));
          await persistSessionName(controller);
        }
        if (trailing) void requestSessionNameRefresh(controller);
        return;
      }
    }
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
    const nameResponseIsCurrent =
      pendingResult.dispatchSnapshot?.sessionNameRevision ===
      controller.sessionNameRevision;
    if (
      !sessionChanged &&
      nameResponseIsCurrent &&
      !controller.pendingSessionRename
    ) {
      applySessionName(controller, reportedSessionName);
    }
    const nowStreaming = data.isStreaming === true;
    const promptTriggeredReplacement =
      sessionChanged && (resolvesRunState || resolvesAdmissionState);
    const replacementPromptRows = promptTriggeredReplacement
      ? controller.messages.filter(
          (entry) =>
            entry.kind === 'user' &&
            (entry.pending === true || Boolean(entry.pendingUserEvent)),
        )
      : [];
    if (resolvesAdmissionState && controller.submittedPrompt) {
      controller.submittedPrompt.admissionStateRequestId = '';
    }
    if (data.isCompacting === true) controller.compacting = true;
    else if (data.isCompacting === false || !nowStreaming) {
      controller.compacting = false;
    }
    if (sessionChanged) controller.retry = undefined;
    clearRemoteConnectionWatch(controller);
    finishRemoteConnection(controller);

    if (sessionChanged) resetQueue(controller, true, true);
    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom && !unsavedSession) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }
    if (sessionChanged) applySessionName(controller, reportedSessionName);

    let syncingAfterSessionChange = false;
    const transfersCommandRefresh =
      sessionChanged &&
      Boolean(
        controller.commandSyncRequestId || controller.commandMessagesRequestId,
      );
    if (sessionChanged) {
      controller.provisionalReplacement =
        transfersCommandRefresh ||
        resolvesCommandSync ||
        Boolean(controller.commandPromptRequestId)
          ? undefined
          : {
              generation: controller.generation,
              sessionId: controller.sessionId,
              sessionPath: controller.sessionPath,
            };
    }
    if (sessionChanged) {
      // The response that revealed the replacement already ended its own
      // span above; anything else still pending for this runtime and
      // generation belongs to the session Pi just swapped out from under it.
      abandonPendingRpcSpans(
        controller.runtimeId,
        controller.generation,
        'abandoned_replacement',
        controller.commandPromptRequestId,
      );
      clearSessionReplacementWatch(controller);
      trailingSessionNameRefresh =
        controller.sessionNameRevision > sessionNameRevisionAtResponse;
      if (controller.sessionNameStateRequestId) {
        clearSessionNameRefresh(controller);
      }
      controller.pendingSessionRename = undefined;
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
      controller.compactionReconciliationPending = false;
      controller.compactionStreamSequence = controller.streamSequence;
      rebindEphemeralSession(controller);
      // A prompt-start state read can reveal the identity Pi created for the
      // active turn. Carry only rows whose provenance ties them to that live
      // prompt; the replacement hydration reconciles them without exposing
      // any completed transcript from the outgoing identity.
      controller.messages = replacementPromptRows;
      resetHistory(controller);
      controller.models = [];
      controller.efforts = [];
      controller.commands = [];
      controller.commandsLoaded = false;
      controller.pendingEffort = '';
      controller.pendingSettingRequestId = '';
      clearSettingRequestWatch(controller);
      syncingAfterSessionChange = true;
      if (trailingSessionNameRefresh) {
        void requestSessionNameRefresh(controller);
      }
    }
    controller.queueSteeringMode = stringValue(data.steeringMode);
    controller.queueFollowUpMode = stringValue(data.followUpMode);
    const materializationBarrierForCurrentSession =
      resolvesMaterializationBarrier && !sessionChanged && !unsavedSession;
    if (materializationBarrierForCurrentSession) {
      clearMaterializationVerificationWatch(controller);
      controller.materializationVerified = true;
      controller.provisionalReplacement = undefined;
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
            Boolean(controller.commandPromptRequestId) ||
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
      const registration = registerConnectedSession(
        controller,
        undefined,
        sessionChanged ||
          Boolean(resolvesPending) ||
          resolvesCommandSync ||
          resolvesMaterializationBarrier,
      );
      if (resolvesBootstrap && !resolvesPending) {
        // Hydration is a Pi read, not a storage write. Its dispatch must not
        // wait for registration, while the identity check below still fences
        // the response against a replacement.
        void registration;
      } else {
        await registration;
      }
    }
    if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
    if (resolvesCommandSync && controller.commandSyncRequestId !== responseId)
      return;
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
    if (resolvesCommandSync && controller.streaming && !sessionChanged) {
      clearCommandRefreshWatch(controller);
      return;
    }
    // A run that outlived its abort is still writing the transcript, so the
    // probe only reports on it. A run that did stop falls through and syncs.
    if (resolvesAbortProbe && controller.streaming && !sessionChanged) return;

    const messagesRequestId = nextRequestId('messages');
    if (resolvesCommandSync || transfersCommandRefresh) {
      controller.commandSyncRequestId = '';
      controller.commandMessagesRequestId = messagesRequestId;
      if (sessionChanged)
        controller.commandSyncingRequestId = messagesRequestId;
    }
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
    await rpc(controller, { id: messagesRequestId, type: 'get_messages' });
    if (!controllerIdentityMatches(controller, synchronizedIdentity)) return;
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
    await rpc(controller, {
      id: nextRequestId('efforts'),
      type: 'get_available_thinking_levels',
    });
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
        controller.provisionalReplacement = undefined;
      } else {
        scheduleMaterializationVerificationRetry(controller);
      }
    }
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.messagesRequestId) &&
      responseId === pending?.messagesRequestId;
    const previous = controller.messages;
    const previousTail = previous.slice(controller.historyPrefixLength);
    const resolvesStart = resolvesStartMessages;
    controller.messagesLoaded = true;
    const hydrated = hydrateTranscript(
      piMessages,
      previousTail,
      controller.streaming || resolvesPending,
    );
    const reconcilingCompaction = controller.compactionReconciliationPending;
    const reconciled = reconcilingCompaction
      ? mergeLiveStreamSuffix(
          hydrated,
          previousTail,
          controller.compactionStreamSequence,
        )
      : resolvesStart && controller.reconnectingRemote
        ? mergeLiveStreamSuffix(hydrated, previousTail, 0)
        : hydrated;
    if (reconcilingCompaction) {
      reanchorCompactionLocalEntries(
        reconciled,
        previousTail,
        controller.localErrors,
        controller.compactionStreamSequence,
      );
    }
    const tail = mergeLocalEntries(
      reconciled,
      controller.localErrors,
      previousTail,
    );
    applyHistoryTail(controller, tail);
    controller.compactionReconciliationPending = false;
    controller.compactionStreamSequence = controller.streamSequence;
    const resolvesAdmission =
      Boolean(controller.submittedPrompt?.admissionMessagesRequestId) &&
      responseId === controller.submittedPrompt?.admissionMessagesRequestId;
    if (resolvesStart) {
      controller.startMessagesRequestId = '';
      if (controller.reconnectingRemote) {
        controller.remoteDisconnected = false;
        controller.reconnectingRemote = false;
        resolveRemoteFeedback(controller);
      }
    }
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
    if (controller.commandMessagesRequestId === responseId) {
      clearCommandRefreshWatch(controller);
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
      await requestPendingMessages(controller);
      return;
    }
    if (resolvesPendingSetting) {
      controller.currentEffort = controller.pendingEffort || 'off';
      controller.pendingEffort = '';
      controller.pendingSettingRequestId = '';
      clearSettingRequestWatch(controller);
    }
  }
}

export default handleResponse;
