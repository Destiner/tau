import {
  draftTitle,
  sessionTitleMarkdown,
  ephemeralSessionByController,
  extensionDialogTimeouts,
  firstUserMessage,
  isControllerSelected,
  state,
  type ExtensionDialog,
  type ExtensionDialogMethod,
  type SessionController,
} from '../../../composables/state';
import { stringValue, type TranscriptNoticeType } from '../transcript';

import { anchorFields } from './history';
import { restoreSubmittedDraft } from './prompts';

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
    if (session?.phantom) {
      session.title = draftTitle(request.text);
      session.titleMarkdown = sessionTitleMarkdown(request.text);
    }
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
    // The directory also resolves relative paths copied from remote sessions;
    // copyPaths still prevents Tau from opening those paths on this machine.
    ...(project ? { workingDirectory: project.workingDirectory } : {}),
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

function recoverControllerDialogDrafts(
  controller: SessionController,
  generation: number,
): void {
  for (const dialog of state.extensionDialogs) {
    if (
      dialog.controllerKey !== controller.key ||
      dialog.generation !== generation ||
      (dialog.method !== 'input' && dialog.method !== 'editor') ||
      !dialog.draft.trim()
    ) {
      continue;
    }
    restoreSubmittedDraft(controller, dialog.draft);
  }
}

export {
  handleExtensionUIRequest,
  isExtensionDialogMethod,
  extensionDialogTitle,
  extensionNotifyType,
  extensionRequestOrigin,
  extensionRequestKey,
  discardExtensionDialog,
  discardControllerDialogs,
  recoverControllerDialogDrafts,
};
