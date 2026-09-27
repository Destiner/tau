import {
  setControllerError,
  setControllerLifecycle,
  type SessionController,
} from '../../../composables/state';

const confirmedAdmissionRequestIds = new Set<string>();

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

function restoreSubmittedDraft(
  controller: SessionController,
  submittedDraft: string,
): void {
  if (controller.draft.trim() === submittedDraft.trim()) return;
  controller.draft = controller.draft
    ? `${submittedDraft}\n\n${controller.draft}`
    : submittedDraft;
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
  publishError = true,
): void {
  const prompt = controller.pendingPrompt;
  if (!prompt) {
    if (publishError) setControllerError(controller, message);
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
  if (publishError) setControllerError(controller, message);
}

export {
  confirmedAdmissionRequestIds,
  appendOptimisticPrompt,
  skillInvocation,
  invokesExtensionCommand,
  restoreSubmittedDraft,
  confirmSubmittedPrompt,
  releaseSubmittedPrompt,
  settleInterruptedSubmittedPrompt,
  recoverSubmittedPrompt,
  cancelPendingPrompt,
};
