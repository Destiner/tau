import {
  markUserMessageSubmitted,
  nextRequestId,
  setControllerError,
  setControllerLifecycle,
  state,
  type SessionController,
} from '../../../composables/state';
import { errorCopy } from '../../error-copy';
import type { TraceContext } from '../../telemetry/trace-context';

import {
  rpc,
  startController,
  clearSessionReplacementWatch,
  clearMaterializationVerificationWatch,
} from './lifecycle';
import {
  appendOptimisticPrompt,
  recoverSubmittedPrompt,
  restoreSubmittedDraft,
  cancelPendingPrompt,
  releaseSubmittedPrompt,
} from './prompts';

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
  if (!prompt.command) markUserMessageSubmitted(controller);
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

export {
  sendPhantomMessage,
  reconcileSubmittedPrompt,
  applyPendingSessionSettings,
  applyPendingSessionEffort,
  requestPendingMessages,
  dispatchPendingPrompt,
};
