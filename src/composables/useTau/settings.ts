import { errorCopy } from '../../lib/error-copy';
import type { ThinkingLevel } from '../../lib/pi/model-scope';
import {
  applySessionName,
  clearSettingRequestWatch,
  requestSessionNameRefresh,
  rpc,
  watchSettingRequest,
} from '../../lib/pi/runtime';
import { startActionSpan } from '../../lib/telemetry';
import {
  activeController,
  canRenameSession,
  nextRequestId,
  normalizeSessionName,
  sessionTitle,
  tooltipTitleMarkdown,
  activeSession,
  setControllerError,
  settingsDisabled,
} from '../state';

import controllerTelemetryScope from './telemetry-scope';

async function selectModel(value: string): Promise<void> {
  const controller = activeController.value;
  const model = controller?.models.find(
    (option) => `${option.provider}/${option.id}` === value,
  );
  if (!controller || !model || settingsDisabled.value) return;
  const actionSpan = startActionSpan(
    'model.select',
    controllerTelemetryScope(controller),
  );
  try {
    if (controller.phantom) {
      controller.currentModelProvider = model.provider;
      controller.currentModelId = model.id;
      controller.currentModelName = model.name;
      return;
    }
    const requestId = nextRequestId('set-model');
    controller.pendingSettingRequestId = requestId;
    watchSettingRequest(controller);
    try {
      await rpc(
        controller,
        {
          id: requestId,
          type: 'set_model',
          provider: model.provider,
          modelId: model.id,
        },
        actionSpan.context,
      );
    } catch {
      if (controller.pendingSettingRequestId === requestId) {
        controller.pendingSettingRequestId = '';
        clearSettingRequestWatch(controller);
      }
      setControllerError(controller, errorCopy.modelChange);
    }
  } finally {
    actionSpan.end();
  }
}

async function renameSession(name: string): Promise<void> {
  const controller = activeController.value;
  if (!controller || !canRenameSession.value) return;
  const next = normalizeSessionName(name);
  if (!next || next === controller.sessionName) return;
  const actionSpan = startActionSpan(
    'session.rename',
    controllerTelemetryScope(controller),
  );
  try {
    const requestId = nextRequestId('session-name');
    controller.pendingSessionRename = {
      requestId,
      previousName: controller.sessionName,
      previousTitle: sessionTitle.value,
      previousTitleMarkdown: activeSession.value
        ? tooltipTitleMarkdown(activeSession.value)
        : undefined,
    };
    controller.sessionNameRevision += 1;
    applySessionName(controller, next);
    try {
      await rpc(
        controller,
        {
          id: requestId,
          type: 'set_session_name',
          name: next,
        },
        actionSpan.context,
      );
    } catch {
      const pending = controller.pendingSessionRename;
      if (pending?.requestId === requestId) {
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
      setControllerError(controller, errorCopy.sessionRename);
    }
  } finally {
    actionSpan.end();
  }
}

async function selectEffort(level: ThinkingLevel): Promise<void> {
  const controller = activeController.value;
  if (
    !controller ||
    !controller.efforts.includes(level) ||
    settingsDisabled.value
  ) {
    return;
  }
  const actionSpan = startActionSpan(
    'effort.select',
    controllerTelemetryScope(controller),
  );
  try {
    if (controller.phantom) {
      controller.currentEffort = level;
      return;
    }
    const requestId = nextRequestId('set-effort');
    controller.pendingEffort = level;
    controller.pendingSettingRequestId = requestId;
    watchSettingRequest(controller);
    try {
      await rpc(
        controller,
        {
          id: requestId,
          type: 'set_thinking_level',
          level,
        },
        actionSpan.context,
      );
    } catch {
      if (controller.pendingSettingRequestId === requestId) {
        controller.pendingSettingRequestId = '';
        controller.pendingEffort = '';
        clearSettingRequestWatch(controller);
      }
      setControllerError(controller, errorCopy.effortChange);
    }
  } finally {
    actionSpan.end();
  }
}

export { selectModel, renameSession, selectEffort };
