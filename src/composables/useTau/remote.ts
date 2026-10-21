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

import {
  beginNavigation,
  currentLifecycle,
  isCurrentNavigation,
} from './navigation';
import { mergeImportedProject } from './projects';
import { writeWorkspace } from './workspace-writes';

let dialogRevision = 0;
let openingIntent = 0;
function dialogOwner(): () => boolean {
  const revision = dialogRevision;
  const lifecycle = currentLifecycle();
  const mode = state.remoteDialogMode;
  return () =>
    revision === dialogRevision &&
    lifecycle === currentLifecycle() &&
    mode === state.remoteDialogMode &&
    state.remoteDialogOpen;
}

function openRemoteProjectDialog(): void {
  if (projectActionsDisabled.value) return;
  openingIntent = beginNavigation();
  dialogRevision += 1;
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
  if (state.remoteDialogOpen) beginNavigation();
  dialogRevision += 1;
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
  const ownsDialog = dialogOwner();
  state.remoteConnecting = true;
  state.remoteConnectionError = '';
  try {
    const listing = await invokeTraced<RemoteDirectoryListing>(
      'probe_remote_project',
      { connectionString },
    );
    if (!ownsDialog()) return;
    applyRemoteDirectoryListing(listing, true);
    state.remoteConnectionString = listing.connectionString;
    state.remoteDialogStep = 'directory';
  } catch {
    if (ownsDialog()) state.remoteConnectionError = errorCopy.remoteConnection;
  } finally {
    if (ownsDialog()) state.remoteConnecting = false;
  }
}

function createRemote(activateProject: (project: ProjectSummary) => void): {
  chooseRemoteDirectory: (
    path: string,
    choice: RemoteDirectoryChoice,
  ) => Promise<void>;
} {
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
    const ownsDialog = dialogOwner();
    const intent = openingIntent;
    state.remoteConnecting = true;
    state.remoteConnectionError = '';
    try {
      if (choice === 'select') {
        const imported = await writeWorkspace<ProjectSummary>(
          'import_remote_project',
          {
            connectionString: state.remoteConnectionString,
            workingDirectory: state.remoteWorkingDirectory,
            host: state.remoteDirectoryHost,
          },
        );
        if (!ownsDialog()) return;
        if (isCurrentNavigation(intent)) {
          const project = mergeImportedProject(imported);
          if (project) activateProject(project);
        }
        state.remoteConnecting = false;
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
        if (!ownsDialog()) return;
        if (choice === 'back') state.remoteDirectoryHistory.pop();
        else state.remoteDirectoryHistory.push(currentDirectory);
        applyRemoteDirectoryListing(listing);
      }
    } catch {
      if (ownsDialog())
        state.remoteConnectionError =
          choice === 'select'
            ? errorCopy.importRemoteProject
            : errorCopy.remoteDirectory;
    } finally {
      if (ownsDialog()) state.remoteConnecting = false;
    }
  }
  return { chooseRemoteDirectory };
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
  createRemote,
  reconnectRemoteSession,
};
