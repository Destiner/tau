import {
  activeExtensionDialog,
  controllerByKey,
  extensionDialogTimeouts,
  nextRequestId,
  setControllerError,
  state,
  type ExtensionDialog,
  type SessionController,
} from '../../../composables/state';
import { errorCopy } from '../../error-copy';
import { TraceContext } from '../../telemetry/trace-context';

import { discardExtensionDialog } from './extensions';
import { setHistoryLoading, revealCachedHistory } from './history';
import {
  rpc,
  clearSessionNameRefresh,
  watchSessionReplacement,
} from './lifecycle';

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

function clearExtensionUiState(): void {
  for (const timeout of extensionDialogTimeouts.values()) clearTimeout(timeout);
  extensionDialogTimeouts.clear();
  for (const controller of state.controllers)
    clearSessionNameRefresh(controller);
  state.extensionDialogs.splice(0);
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
    setControllerError(controller, errorCopy.historyLoad);
  }
}

export {
  submitExtensionDialog,
  cancelExtensionDialog,
  respondToExtensionDialog,
  clearExtensionUiState,
  requestEarlierHistory,
};
