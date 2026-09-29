import {
  controllerByRuntimeId,
  ephemeralSessionByController,
  isControllerSelected,
  nextRequestId,
  presentRemoteConnectionError,
  setControllerError,
  setControllerLifecycle,
  state,
  touchController,
  workspaceContainsSession,
  type SessionController,
} from '../../../composables/state';
import { errorCopy } from '../../error-copy';
import { PiBridgeEvent } from '../bridge';
import { parseQueueSnapshot } from '../queue';
import {
  abandonPendingRpcSpans,
  resetStreamAggregate,
  recordStreamDelta,
  flushStreamAggregate,
} from '../rpc-bookkeeping';
import {
  asRecord,
  contentText,
  localErrorId,
  messageFailure,
  parseSkillBlock,
  projectOrdinaryUserMessage,
  stringValue,
  toolArgumentsText,
  toolResultText,
  toolSummary,
} from '../transcript';

import {
  handleExtensionUIRequest,
  discardControllerDialogs,
  recoverControllerDialogDrafts,
} from './extensions';
import {
  resetHistory,
  setHistoryLoading,
  anchorFields,
  pushError,
  retryStatus,
  appendStream,
} from './history';
import {
  rpc,
  piConnectionFailureMessage,
  piProcessExitMessage,
  remotePiConnectionFailureMessage,
  remotePiProcessExitMessage,
  remoteDisconnectedMessage,
  remoteReconnectFailureMessage,
  clearRemoteConnectionWatch,
  clearSettingRequestWatch,
  clearSessionNameRefresh,
  requestSessionNameRefresh,
  watchSessionReplacement,
  clearSessionReplacementWatch,
  clearCommandRefreshWatch,
  clearMaterializationVerificationWatch,
  clearAbortWatch,
  discardReleasedEmptySession,
} from './lifecycle';
import {
  confirmSubmittedPrompt,
  settleInterruptedSubmittedPrompt,
  cancelPendingPrompt,
} from './prompts';
import { resetQueue } from './queue-state';
import {
  hasMeaningfulAssistantActivity,
  markPiTranscript,
} from './registration';
import handleResponse from './responses';

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
      clearSessionNameRefresh(controller);
      flushStreamAggregate(controller.runtimeId, controller.generation);
      discardControllerDialogs(controller);
      controller.retry = undefined;
      resetQueue(controller, true);
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
    controller.retry = undefined;
    const project = state.workspace?.projects.find(
      (item) => item.path === controller.projectPath,
    );
    const message = project?.connectionString
      ? controller.reconnectingRemote
        ? remoteReconnectFailureMessage
        : controller.connectingRemote
          ? remotePiConnectionFailureMessage
          : remoteDisconnectedMessage
      : piConnectionFailureMessage;
    if (controller.connectingRemote) {
      clearRemoteConnectionWatch(controller);
      if (controller.pendingPrompt)
        cancelPendingPrompt(controller, message, false);
      if (controller.reconnectingRemote) {
        setControllerError(controller, remoteReconnectFailureMessage);
      } else {
        presentRemoteConnectionError(controller, message);
      }
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
    clearSessionNameRefresh(controller);
    clearMaterializationVerificationWatch(controller);
    clearAbortWatch(controller);
    clearRemoteConnectionWatch(controller);
    const project = state.workspace?.projects.find(
      (item) => item.path === controller.projectPath,
    );
    const remoteProject = Boolean(project?.connectionString);
    const initialRemoteConnectionFailed =
      (controller.connectingRemote && !controller.reconnectingRemote) ||
      state.remoteRetry?.controllerKey === controller.key;
    if (initialRemoteConnectionFailed) {
      discardControllerDialogs(controller, event.generation);
      const failure =
        event.code === 0
          ? 'The remote Pi process stopped before it was ready.'
          : remotePiProcessExitMessage;
      if (controller.pendingPrompt) {
        cancelPendingPrompt(controller, failure, false);
      }
      presentRemoteConnectionError(controller, failure);
      return;
    }
    const recoverableRemoteExit = remoteProject;
    const message = recoverableRemoteExit
      ? controller.reconnectingRemote
        ? remoteReconnectFailureMessage
        : remoteDisconnectedMessage
      : event.code === 0
        ? ''
        : piProcessExitMessage;
    if (recoverableRemoteExit) {
      recoverControllerDialogDrafts(controller, event.generation);
    }
    discardControllerDialogs(controller, event.generation);
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message || 'The Pi process stopped.');
    }
    settleInterruptedSubmittedPrompt(controller);
    resetQueue(controller, true);
    for (const entry of controller.messages) {
      if (entry.kind === 'tool' && entry.toolRunning) entry.toolRunning = false;
    }
    if (recoverableRemoteExit && !controller.remoteDisconnected) {
      controller.messages.push({
        id: `remote-interruption-${event.generation}`,
        kind: 'notice',
        text: 'The remote connection was interrupted.',
        noticeType: 'warning',
        anchor: controller.messages.length,
      });
    }
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
        syncing: false,
        connectingRemote: false,
      },
      'process_exited',
    );
    controller.bootstrapStateRequestId = '';
    controller.bootstrapSessionPath = '';
    controller.runStateRequestId = '';
    controller.startMessagesRequestId = '';
    controller.commandPromptRequestId = '';
    clearCommandRefreshWatch(controller);
    controller.replacementProbeRequestId = '';
    controller.abortProbeRequestId = '';
    controller.historyRequestId = '';
    setHistoryLoading(controller, false);
    controller.pendingSessionRename = undefined;
    controller.pendingEffort = '';
    controller.pendingSettingRequestId = '';
    controller.remoteConnectionTimedOut = false;
    clearSettingRequestWatch(controller);
    controller.generation = 0;
    controller.remoteDisconnected = recoverableRemoteExit;
    controller.reconnectingRemote = false;
    controller.retry = undefined;
    if (message) setControllerError(controller, message);
    if (recoverableRemoteExit && !isControllerSelected(controller)) {
      controller.unread = true;
    }
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
  if (type === 'queue_update') {
    const snapshot = parseQueueSnapshot(event);
    if (snapshot && controller.ready) {
      controller.queue = snapshot;
      controller.queueVersion += 1;
    }
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
    if (!controller.compactionReconciliationPending) {
      controller.compactionStreamSequence = controller.streamSequence;
    }
    setControllerLifecycle(
      controller,
      { streaming: true, stopping: false, working: true },
      'agent_start',
    );
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
      // Pi has persisted the compaction and rebuilt its message list before
      // this event. Reconcile from that source now: the same run may continue
      // for a long time before settlement. Feedback already shown belongs to
      // the transcript Pi just replaced and must not be carried into it.
      const localErrorIds = new Set(
        controller.localErrors.map((error) => localErrorId(error.key)),
      );
      controller.messages = controller.messages.filter(
        (entry) => entry.kind !== 'notice' && !localErrorIds.has(entry.id),
      );
      controller.localErrors = [];
      resetHistory(controller);
      controller.compactionReconciliationPending = true;
      controller.compactionStreamSequence = controller.streamSequence;
      try {
        await rpc(controller, {
          id: nextRequestId('compaction-messages'),
          type: 'get_messages',
        });
      } catch {
        // Settlement retries the authoritative hydration. Keep post-compaction
        // stream rows separate from the stale projection until then.
      }
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
    controller.retry = undefined;
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
  // Pi's notification has no session identity. Treat it as an invalidation:
  // an extension may already have replaced the session behind this runtime.
  if (type === 'session_info_changed') {
    controller.sessionNameRevision += 1;
    await requestSessionNameRefresh(controller);
    return;
  }
  if (type === 'auto_retry_start') {
    controller.retry = {
      generation: controller.generation,
      attempt: Math.max(1, Number(event.attempt) || 1),
      reason: retryStatus(event),
    };
    return;
  }
  if (type === 'auto_retry_end') {
    controller.retry = undefined;
    return;
  }
  if (type === 'extension_error') {
    setControllerError(controller, errorCopy.extensionFailure);
  }
}

export { handleBridgeEvent, handleRpc };
