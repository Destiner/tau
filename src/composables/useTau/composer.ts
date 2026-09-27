import { errorCopy } from '../../lib/error-copy';
import {
  appendOptimisticPrompt,
  clearAbortWatch,
  clearPendingQueue,
  clearSessionReplacementWatch,
  invokesExtensionCommand,
  requestEarlierHistory,
  submitExtensionDialog as sendExtensionDialogResponse,
  cancelExtensionDialog as sendExtensionDialogCancellation,
  recoverSubmittedPrompt,
  registerConnectedSession,
  rpc,
  sendPhantomMessage,
  submitQueuedMessage,
  watchAbort,
} from '../../lib/pi/runtime';
import { startActionSpan } from '../../lib/telemetry';
import {
  activeController,
  projectActionsDisabled,
  canCompose,
  canQueue,
  markUserMessageSubmitted,
  nextRequestId,
  setControllerError,
  setControllerLifecycle,
  state,
  type ProjectSummary,
  type SessionSummary,
} from '../state';

import controllerTelemetryScope from './telemetry-scope';

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function createComposer(
  unarchiveSession: (
    project: ProjectSummary,
    session: SessionSummary,
  ) => Promise<void>,
) {
  async function sendMessage(
    intent: 'steer' | 'followUp' = 'steer',
  ): Promise<void> {
    const controller = activeController.value;
    const submittedDraft = controller?.draft ?? '';
    const message = submittedDraft.trim();
    if (controller?.streaming) {
      if (
        !message ||
        !canQueue.value ||
        invokesExtensionCommand(controller, message)
      )
        return;
      markUserMessageSubmitted(controller);
      const project = state.workspace?.projects.find(
        (candidate) => candidate.path === controller.projectPath,
      );
      const sessionRecord = project?.sessions.find(
        (candidate) => candidate.id === controller.sessionId,
      );
      if (project && sessionRecord?.archived)
        void unarchiveSession(project, sessionRecord);
      await submitQueuedMessage(controller, submittedDraft, intent);
      return;
    }
    if (
      !controller ||
      !message ||
      !canCompose.value ||
      controller.streaming ||
      controller.stopping ||
      controller.working ||
      controller.promptSubmitting ||
      projectActionsDisabled.value
    ) {
      return;
    }

    const actionSpan = startActionSpan(
      'message.send',
      controllerTelemetryScope(controller),
    );
    try {
      const command = invokesExtensionCommand(controller, message);
      if (!command && !controller.phantom) {
        markUserMessageSubmitted(controller);
      }

      controller.promptSubmitting = true;
      controller.queueFeedback = '';
      controller.draft = '';

      if (controller.phantom) {
        await sendPhantomMessage(
          controller,
          message,
          submittedDraft,
          command,
          actionSpan.context,
        );
        return;
      }

      const optimisticId = `optimistic-user-${Date.now()}`;
      const requestId = nextRequestId('prompt');
      if (command) controller.commandPromptRequestId = requestId;
      const submission = {
        requestId,
        generation: controller.generation,
        message,
        draft: submittedDraft,
        accepted: false,
        ...(command ? {} : { optimisticId }),
      };
      controller.submittedPrompt = submission;
      clearSessionReplacementWatch(controller);
      setControllerLifecycle(
        controller,
        { working: true },
        'message_send',
        actionSpan.context,
      );
      if (!command) {
        appendOptimisticPrompt(controller, message, optimisticId);
      }
      try {
        // Sending to a session whose record is still archived is an implicit
        // unarchive: activity proves the session is wanted again.
        const project = state.workspace?.projects.find(
          (candidate) => candidate.path === controller.projectPath,
        );
        const sessionRecord = project?.sessions.find(
          (candidate) => candidate.id === controller.sessionId,
        );
        if (project && sessionRecord?.archived) {
          void unarchiveSession(project, sessionRecord);
        }
        await rpc(
          controller,
          { id: requestId, type: 'prompt', message },
          actionSpan.context,
        );
      } catch {
        if (controller.submittedPrompt?.requestId !== submission.requestId)
          return;
        controller.promptSubmitting = false;
        if (submission.accepted) {
          controller.submittedPrompt = undefined;
          return;
        }
        recoverSubmittedPrompt(controller);
        setControllerLifecycle(
          controller,
          { working: false },
          'message_send_failed',
          actionSpan.context,
        );
        controller.commandPromptRequestId = '';
        setControllerError(controller, errorCopy.messageSend);
        return;
      }
      if (
        controller.submittedPrompt?.requestId !== submission.requestId &&
        !submission.accepted
      ) {
        return;
      }
      controller.promptSubmitting = false;
      if (!command) {
        await registerConnectedSession(controller, actionSpan.context);
      }
    } finally {
      actionSpan.end();
    }
  }

  async function clearQueue(): Promise<void> {
    const controller = activeController.value;
    if (controller) await clearPendingQueue(controller);
  }

  function dismissQueueFeedback(): void {
    const controller = activeController.value;
    if (controller) controller.queueFeedback = '';
  }

  function recoverQueueDraft(index: number): void {
    const controller = activeController.value;
    if (!controller || controller.draft || !controller.queueFailedDrafts[index])
      return;
    controller.draft = controller.queueFailedDrafts[index];
    controller.queueFailedDrafts.splice(index, 1);
    controller.queueFeedback = '';
  }

  async function stop(): Promise<void> {
    const controller = activeController.value;
    if (
      !controller?.streaming ||
      controller.compacting ||
      controller.stopping ||
      projectActionsDisabled.value
    )
      return;
    const actionSpan = startActionSpan(
      'session.stop',
      controllerTelemetryScope(controller),
    );
    try {
      setControllerLifecycle(
        controller,
        { stopping: true },
        'stop_requested',
        actionSpan.context,
      );
      watchAbort(controller);
      try {
        await rpc(
          controller,
          { id: nextRequestId('abort'), type: 'abort' },
          actionSpan.context,
        );
      } catch {
        clearAbortWatch(controller);
        setControllerLifecycle(
          controller,
          { stopping: false },
          'stop_failed',
          actionSpan.context,
        );
        setControllerError(controller, errorCopy.stopWork);
      }
    } finally {
      actionSpan.end();
    }
  }
  async function loadEarlierHistory(): Promise<void> {
    const controller = activeController.value;
    if (!controller || projectActionsDisabled.value) return;
    await requestEarlierHistory(controller);
  }

  async function submitExtensionDialog(value: string | boolean): Promise<void> {
    const controller = activeController.value;
    if (!controller || projectActionsDisabled.value) return;
    const actionSpan = startActionSpan(
      'extension.dialog.submit',
      controllerTelemetryScope(activeController.value),
    );
    try {
      await sendExtensionDialogResponse(value, actionSpan.context);
    } finally {
      actionSpan.end();
    }
  }

  async function cancelExtensionDialog(): Promise<void> {
    const controller = activeController.value;
    if (!controller || projectActionsDisabled.value) return;
    const actionSpan = startActionSpan(
      'extension.dialog.cancel',
      controllerTelemetryScope(activeController.value),
    );
    try {
      await sendExtensionDialogCancellation(actionSpan.context);
    } finally {
      actionSpan.end();
    }
  }

  return {
    sendMessage,
    clearQueue,
    dismissQueueFeedback,
    recoverQueueDraft,
    stop,
    loadEarlierHistory,
    submitExtensionDialog,
    cancelExtensionDialog,
  };
}

export default createComposer;
