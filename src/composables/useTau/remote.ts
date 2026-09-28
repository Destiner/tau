import { errorCopy } from '../../lib/error-copy';
import { cancelPendingPrompt, startController } from '../../lib/pi/runtime';
import { invokeTraced } from '../../lib/telemetry';
import {
  activeController,
  activeProject,
  projectActionsDisabled,
  applyRemoteDirectoryListing,
  canReconnectRemote,
  clearRemoteDirectoryBrowser,
  clearRemoteRetry,
  controllerByKey,
  state,
  type ProjectSummary,
  type RemoteDirectoryChoice,
  type RemoteDirectoryListing,
} from '../state';

import { mergeImportedProject } from './projects';

function openRemoteProjectDialog(): void {
  if (projectActionsDisabled.value) return;
  clearRemoteRetry();
  state.remoteDialogMode = 'add';
  state.remoteDialogStep = 'connection';
  state.remoteConnectionString = '';
  state.remoteConnectionError = '';
  state.remoteConnecting = false;
  clearRemoteDirectoryBrowser();
  state.remoteDialogOpen = true;
}

function closeRemoteProjectDialog(): void {
  if (state.remoteConnecting) return;
  const retryController = state.remoteRetry
    ? controllerByKey(state.remoteRetry.controllerKey)
    : undefined;
  if (state.remoteDialogMode === 'retry' && retryController?.pendingPrompt) {
    cancelPendingPrompt(retryController, '');
  }
  clearRemoteRetry();
  state.remoteDialogOpen = false;
  state.remoteConnectionError = '';
  clearRemoteDirectoryBrowser();
}

async function submitRemoteConnection(): Promise<void> {
  if (projectActionsDisabled.value || state.remoteConnecting) return;
  if (state.remoteDialogMode === 'retry') {
    await retryRemoteConnection();
    return;
  }

  const connectionString = state.remoteConnectionString.trim();
  if (!connectionString) {
    state.remoteConnectionError = 'Enter an SSH connection string.';
    return;
  }
  state.remoteConnecting = true;
  state.remoteConnectionError = '';
  try {
    const listing = await invokeTraced<RemoteDirectoryListing>(
      'probe_remote_project',
      { connectionString },
    );
    applyRemoteDirectoryListing(listing, true);
    state.remoteConnectionString = listing.connectionString;
    state.remoteDialogStep = 'directory';
  } catch {
    state.remoteConnectionError = errorCopy.remoteConnection;
  } finally {
    state.remoteConnecting = false;
  }
}

async function chooseRemoteDirectory(
  path: string,
  choice: RemoteDirectoryChoice,
): Promise<void> {
  if (
    projectActionsDisabled.value ||
    state.remoteConnecting ||
    state.remoteDialogStep !== 'directory'
  ) {
    return;
  }
  state.remoteConnecting = true;
  state.remoteConnectionError = '';
  try {
    if (choice === 'select') {
      mergeImportedProject(
        await invokeTraced<ProjectSummary>('import_remote_project', {
          connectionString: state.remoteConnectionString,
          workingDirectory: state.remoteWorkingDirectory,
          host: state.remoteDirectoryHost,
        }),
      );
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace?.activeProjectPath ?? '';
      }
      state.remoteDialogOpen = false;
      clearRemoteDirectoryBrowser();
    } else {
      const currentDirectory = state.remoteWorkingDirectory;
      const listing = await invokeTraced<RemoteDirectoryListing>(
        'list_remote_directories',
        {
          connectionString: state.remoteConnectionString,
          workingDirectory: path,
        },
      );
      if (choice === 'back') state.remoteDirectoryHistory.pop();
      else state.remoteDirectoryHistory.push(currentDirectory);
      applyRemoteDirectoryListing(listing);
    }
  } catch {
    state.remoteConnectionError =
      choice === 'select'
        ? errorCopy.importRemoteProject
        : errorCopy.remoteDirectory;
  } finally {
    state.remoteConnecting = false;
  }
}

async function retryRemoteConnection(): Promise<void> {
  if (projectActionsDisabled.value) return;
  const retry = state.remoteRetry;
  const controller = retry ? controllerByKey(retry.controllerKey) : undefined;
  const project = state.workspace?.projects.find(
    (item) => item.path === retry?.projectPath,
  );
  if (!retry || !controller || !project?.connectionString) {
    state.remoteConnectionError = errorCopy.remoteSessionUnavailable;
    return;
  }

  state.remoteConnecting = true;
  state.remoteConnectionError = '';
  try {
    await startController(
      controller,
      project,
      retry.sessionPath,
      retry.preserveMessages,
    );
  } catch {
    state.remoteConnecting = false;
    state.remoteConnectionError = errorCopy.remoteConnection;
  }
}

async function reconnectRemoteSession(): Promise<void> {
  if (projectActionsDisabled.value) return;
  const controller = activeController.value;
  const project = activeProject.value;
  if (!controller || !project || !canReconnectRemote.value) return;

  controller.reconnectingRemote = true;
  await startController(
    controller,
    project,
    controller.sessionPath || undefined,
    true,
  );
}

export {
  openRemoteProjectDialog,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  reconnectRemoteSession,
};
