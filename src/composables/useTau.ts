import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

import { errorCopy } from '../lib/error-copy';
import type { PiBridgeEvent } from '../lib/pi/bridge';
import type { ThinkingLevel } from '../lib/pi/model-scope';
import {
  applySessionName,
  appendOptimisticPrompt,
  cancelExtensionDialog as sendExtensionDialogCancellation,
  cancelPendingPrompt,
  clearAbortWatch,
  clearExtensionUiState,
  clearRemoteConnectionWatch,
  clearSessionReplacementWatch,
  clearSettingRequestWatch,
  discardUnregisteredEphemeralSession,
  handleBridgeEvent,
  invokesExtensionCommand,
  persistExpandedProject,
  persistProjectSelection,
  recoverSubmittedPrompt,
  registerConnectedSession,
  requestEarlierHistory,
  releaseIdleRuntimes,
  releaseRuntime,
  removeEmptyActivePhantom,
  removeProjectUiState,
  removeRegisteredEphemeralSession,
  rpc,
  sendPhantomMessage,
  startController,
  stopControllerProcess,
  submitExtensionDialog as sendExtensionDialogResponse,
  watchAbort,
  watchSettingRequest,
} from '../lib/pi/runtime';
import {
  invokeTraced,
  startActionSpan,
  type TelemetryScope,
} from '../lib/telemetry';

import {
  activeController,
  activeExtensionDialog,
  activeProject,
  projectActionsDisabled,
  applyRemoteDirectoryListing,
  canArchiveSession,
  canCompose,
  canDraft,
  canRenameSession,
  clearActiveSession,
  clearControllerActionError,
  clearRemoteDirectoryBrowser,
  clearRemoteRetry,
  clearWorkspaceError,
  commands,
  controllerByKey,
  controllerByRuntimeId,
  controllerForSession,
  createController,
  createPhantomSession,
  currentEffort,
  currentEffortLabel,
  currentModelId,
  currentModelLabel,
  currentModelProvider,
  draft,
  effortLabels,
  efforts,
  ensureController,
  indicatorLabel,
  inheritControllerSettings,
  isProjectRemoving,
  isSessionSelected,
  isSessionUnread,
  markSessionRead,
  markSessionUnread,
  markUserMessageSubmitted,
  messages,
  models,
  nextControllerKey,
  nextRequestId,
  normalizeSessionName,
  projectIndicator,
  projectSessions,
  promptSubmitting,
  archivedSessionEntries,
  relativeTimestamp,
  runtimeAvailable,
  sessionLastUserMessageAt,
  sessionSortAt,
  sessionIndicator,
  sessionLastActive,
  sessionLoading,
  sessionTitle,
  setActiveSessionView,
  setControllerActionError,
  setControllerError,
  setControllerLifecycle,
  setWorkspaceError,
  settingsDisabled,
  state,
  status,
  stopping,
  streaming,
  type ProjectSummary,
  type RemoteDirectoryChoice,
  type RemoteDirectoryListing,
  type SessionController,
  type SessionSummary,
  type WorkspaceSnapshot,
} from './state';

let unlisten: UnlistenFn | undefined;

function controllerTelemetryScope(
  controller: SessionController | undefined,
  sessionId?: string,
): TelemetryScope {
  return {
    sessionId: controller?.sessionId || sessionId,
    controllerId: controller?.key,
    runtimeId: controller?.runtimeId,
    generation: controller?.generation || undefined,
  };
}

// The composable returns its own surface: about sixty refs and handlers whose
// types are all inferred, so spelling the shape out would only duplicate them.
// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function useTau() {
  async function initialize(): Promise<void> {
    if (!unlisten) {
      unlisten = await listen<PiBridgeEvent>('pi-event', ({ payload }) => {
        void handleBridgeEvent(payload).catch(() => {
          const controller = controllerByRuntimeId(payload.runtimeId);
          if (!controller) return;
          setControllerLifecycle(
            controller,
            { syncing: false },
            'bridge_event_failed',
          );
          setControllerError(controller, errorCopy.bridgeEvent);
          releaseRuntime(controller);
        });
      });
    }
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'load_workspace',
        {},
      );
      state.workspaceStatus = '';
      state.activeProjectPath = state.workspace.activeProjectPath;
      const selectedProject = state.workspace.projects.find(
        (project) => project.selected,
      );
      const selectedSession = selectedProject?.sessions.find(
        (session) => session.selected,
      );
      if (selectedProject && selectedSession) {
        const controller = ensureController(selectedProject, selectedSession);
        setActiveSessionView(selectedProject, selectedSession, controller);
        const needsLocalRuntime = !selectedProject.connectionString;
        if (needsLocalRuntime && !state.workspace.piPath) {
          controller.status = 'Pi was not found. Install Pi, then restart Tau.';
          return;
        }
        await startController(
          controller,
          selectedProject,
          selectedSession.path,
        );
      }
    } catch {
      const controller = activeController.value;
      if (controller) {
        setControllerLifecycle(
          controller,
          { starting: false },
          'workspace_load_failed',
        );
        setControllerError(controller, errorCopy.loadWorkspace);
      } else {
        setWorkspaceError(errorCopy.loadWorkspace);
      }
    }
  }

  function dispose(): void {
    unlisten?.();
    unlisten = undefined;
    for (const controller of state.controllers) {
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      clearRemoteConnectionWatch(controller);
      clearSettingRequestWatch(controller);
    }
    clearExtensionUiState();
  }

  function submitIssueReport(
    description: string,
    sessionId?: string,
  ): Promise<void> {
    return invokeTraced('submit_issue_report', {
      description,
      ...(sessionId ? { sessionId } : {}),
    });
  }

  async function addLocalProject(): Promise<void> {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
    let selection: string | null;
    try {
      selection = await open({
        directory: true,
        multiple: false,
        title: 'Choose a Project Folder',
      });
    } catch {
      setWorkspaceError(errorCopy.folderPicker);
      return;
    }
    if (!selection) return;
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'import_project',
        { path: selection },
      );
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
    } catch {
      setWorkspaceError(errorCopy.importProject);
    }
  }

  function openRemoteProjectDialog(): void {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
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
    if (state.remoteConnecting) return;
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
    if (state.remoteConnecting || state.remoteDialogStep !== 'directory') {
      return;
    }
    state.remoteConnecting = true;
    state.remoteConnectionError = '';
    try {
      if (choice === 'select') {
        state.workspace = await invokeTraced<WorkspaceSnapshot>(
          'import_remote_project',
          {
            connectionString: state.remoteConnectionString,
            workingDirectory: state.remoteWorkingDirectory,
            host: state.remoteDirectoryHost,
          },
        );
        if (!state.activeProjectPath) {
          state.activeProjectPath = state.workspace.activeProjectPath;
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

  async function toggleProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'set_project_collapsed',
        { path: project.path, collapsed: !project.collapsed },
      );
    } catch {
      setWorkspaceError(errorCopy.sidebarChange);
    }
  }

  async function reorderProjects(
    fromIndex: number,
    toIndex: number,
  ): Promise<void> {
    const workspace = state.workspace;
    if (
      !workspace ||
      state.removingProjectPaths.length > 0 ||
      fromIndex < 0 ||
      fromIndex >= workspace.projects.length ||
      toIndex < 0 ||
      toIndex >= workspace.projects.length ||
      fromIndex === toIndex
    ) {
      return;
    }

    clearWorkspaceError();
    const projects = [...workspace.projects];
    const [project] = projects.splice(fromIndex, 1);
    if (!project) return;
    projects.splice(toIndex, 0, project);
    state.workspace = { ...workspace, projects };

    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'reorder_projects',
        { projectPaths: projects.map((candidate) => candidate.path) },
      );
    } catch {
      state.workspace = workspace;
      setWorkspaceError(errorCopy.projectOrder);
    }
  }

  async function removeProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
    state.removingProjectPaths.push(project.path);
    const projectControllers = state.controllers.filter(
      (controller) => controller.projectPath === project.path,
    );

    try {
      const removingActiveView = project.path === state.activeProjectPath;
      const workspace = await invokeTraced<WorkspaceSnapshot>(
        'remove_project',
        { path: project.path },
      );
      state.workspace = workspace;
      if (removingActiveView && state.activeProjectPath === project.path) {
        clearActiveSession();
      }
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
      for (const controller of projectControllers) controller.disposed = true;
      await Promise.all(
        projectControllers.map((controller) =>
          stopControllerProcess(controller),
        ),
      );
      removeProjectUiState(project.path);
    } catch {
      setWorkspaceError(errorCopy.removeProject);
    } finally {
      state.removingProjectPaths = state.removingProjectPaths.filter(
        (path) => path !== project.path,
      );
    }
  }

  async function archiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (!canArchiveSession(project, session)) return;
    clearWorkspaceError();
    const controller = controllerForSession(project.path, session.id);
    clearControllerActionError(controller);
    const discardedViewWasSelected =
      state.activeProjectPath === project.path &&
      state.activeSessionId === session.id;
    if (controller && discardUnregisteredEphemeralSession(controller)) {
      if (!discardedViewWasSelected) return;
      const updatedProject = state.workspace?.projects.find(
        (candidate) => candidate.path === project.path,
      );
      if (!updatedProject) {
        clearActiveSession();
        return;
      }
      const nextSession = projectSessions(updatedProject)[0];
      if (nextSession) await selectSession(updatedProject, nextSession);
      else await newSession(updatedProject);
      return;
    }

    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'archive_session',
        { projectPath: project.path, sessionId: session.id },
      );
      if (controller) removeRegisteredEphemeralSession(controller);

      const archivedViewStillSelected =
        state.activeProjectPath === project.path &&
        state.activeSessionId === session.id;
      if (!archivedViewStillSelected) {
        releaseRuntime(controller);
        return;
      }

      const updatedProject = state.workspace.projects.find(
        (candidate) => candidate.path === project.path,
      );
      if (!updatedProject) {
        clearActiveSession();
        return;
      }
      const nextSession = projectSessions(updatedProject)[0];
      if (nextSession) await selectSession(updatedProject, nextSession);
      else await newSession(updatedProject);
    } catch {
      const errorController = controller ?? ensureController(project, session);
      setControllerActionError(errorController, errorCopy.archiveSession);
      if (!isSessionSelected(project, session)) errorController.unread = true;
    }
  }

  async function unarchiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'unarchive_session',
        { projectPath: project.path, sessionId: session.id },
      );
      // The record is reachable again, but nothing selects it here: the
      // archived list is a review surface, not a session switcher.
    } catch {
      setWorkspaceError(errorCopy.restoreSession);
    }
  }

  async function newSession(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    clearWorkspaceError();
    const actionSpan = startActionSpan('session.new');
    try {
      const previous = activeController.value;
      removeEmptyActivePhantom();
      const controllerKey = nextControllerKey();
      const session = createPhantomSession(project.path, controllerKey);
      const controller = createController(project, session, controllerKey);
      inheritControllerSettings(controller, previous);
      state.ephemeralSessions.push(session);
      if (project.collapsed) {
        project.collapsed = false;
        void persistExpandedProject(project.path, actionSpan.context);
      }
      setActiveSessionView(project, session, controller);
      controller.status = '';
      await persistProjectSelection(
        project.path,
        controller,
        actionSpan.context,
      );
      releaseIdleRuntimes();
      if (
        runtimeAvailable(project) &&
        (!controller.currentModelId ||
          controller.models.length === 0 ||
          controller.efforts.length === 0 ||
          !controller.commandsLoaded)
      ) {
        await startController(
          controller,
          project,
          undefined,
          false,
          actionSpan.context,
        );
      }
    } finally {
      actionSpan.end();
    }
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    const actionSpan = startActionSpan(
      'session.select',
      controllerTelemetryScope(
        controllerForSession(project.path, session.id),
        session.id,
      ),
    );
    try {
      const alreadySelected =
        project.path === state.activeProjectPath &&
        session.id === state.activeSessionId;
      if (alreadySelected) {
        const controller = activeController.value;
        if (controller) {
          controller.unread = false;
          if (
            !controller.phantom &&
            !controller.ready &&
            !controller.starting
          ) {
            await startController(
              controller,
              project,
              session.path,
              false,
              actionSpan.context,
            );
          }
        }
        return;
      }

      removeEmptyActivePhantom();
      const controller = ensureController(project, session);
      setActiveSessionView(project, session, controller);
      controller.status = '';
      controller.unread = false;
      await persistProjectSelection(
        project.path,
        controller,
        actionSpan.context,
      );
      releaseIdleRuntimes();

      if (controller.phantom || controller.ready || controller.starting) return;
      await startController(
        controller,
        project,
        session.path,
        false,
        actionSpan.context,
      );
    } finally {
      actionSpan.end();
    }
  }

  async function sendMessage(): Promise<void> {
    const controller = activeController.value;
    const message = controller?.draft.trim() ?? '';
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
      if (!command) markUserMessageSubmitted(controller);

      if (controller.phantom) {
        await sendPhantomMessage(
          controller,
          message,
          command,
          actionSpan.context,
        );
        return;
      }

      controller.promptSubmitting = true;

      // Sending to a session whose record is still archived is an implicit
      // unarchive: activity proves the session is wanted again.
      const project = state.workspace?.projects.find(
        (candidate) => candidate.path === controller.projectPath,
      );
      const sessionRecord = project?.sessions.find(
        (candidate) => candidate.id === controller.sessionId,
      );
      if (project && sessionRecord?.archived) {
        await unarchiveSession(project, sessionRecord);
      }

      const optimisticId = `optimistic-user-${Date.now()}`;
      setControllerLifecycle(
        controller,
        { working: true },
        'message_send',
        actionSpan.context,
      );
      if (!command) {
        appendOptimisticPrompt(controller, message, optimisticId);
      }
      controller.status = '';
      try {
        const requestId = nextRequestId('prompt');
        if (command) controller.commandPromptRequestId = requestId;
        controller.submittedPrompt = {
          requestId,
          message,
          ...(command ? {} : { optimisticId }),
        };
        await rpc(
          controller,
          { id: requestId, type: 'prompt', message },
          actionSpan.context,
        );
        controller.promptSubmitting = false;
        if (controller.draft.trim() === message) controller.draft = '';
        if (!command) {
          await registerConnectedSession(controller, actionSpan.context);
        }
      } catch {
        controller.promptSubmitting = false;
        recoverSubmittedPrompt(controller);
        setControllerLifecycle(
          controller,
          { working: false },
          'message_send_failed',
          actionSpan.context,
        );
        controller.commandPromptRequestId = '';
        setControllerError(controller, errorCopy.messageSend);
      }
    } finally {
      actionSpan.end();
    }
  }

  async function stop(): Promise<void> {
    const controller = activeController.value;
    if (
      !controller?.streaming ||
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
      controller.status = '';
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
      controller.status = '';
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
      controller.status = '';
      const requestId = nextRequestId('session-name');
      controller.pendingSessionRename = {
        requestId,
        previousName: controller.sessionName,
        previousTitle: sessionTitle.value,
      };
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
          applySessionName(
            controller,
            pending.previousName,
            pending.previousTitle,
          );
          controller.pendingSessionRename = undefined;
        }
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
      controller.status = '';
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

  async function loadEarlierHistory(): Promise<void> {
    const controller = activeController.value;
    if (!controller) return;
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
    state,
    activeProject,
    projectActionsDisabled,
    activeController,
    messages,
    draft,
    status,
    streaming,
    stopping,
    promptSubmitting,
    models,
    efforts,
    commands,
    activeExtensionDialog,
    currentModelProvider,
    currentModelId,
    currentEffort,
    sessionTitle,
    currentModelLabel,
    currentEffortLabel,
    effortLabels,
    settingsDisabled,
    canDraft,
    canCompose,
    canRenameSession,
    sessionLoading,
    initialize,
    dispose,
    submitIssueReport,
    addLocalProject,
    openRemoteProjectDialog,
    closeRemoteProjectDialog,
    submitRemoteConnection,
    chooseRemoteDirectory,
    toggleProject,
    reorderProjects,
    removeProject,
    archiveSession,
    unarchiveSession,
    newSession,
    selectSession,
    canArchiveSession,
    isProjectRemoving,
    projectSessions,
    archivedSessionEntries,
    sessionLastActive,
    sessionLastUserMessageAt,
    sessionSortAt,
    relativeTimestamp,
    isSessionSelected,
    sessionIndicator,
    isSessionUnread,
    markSessionUnread,
    markSessionRead,
    projectIndicator,
    indicatorLabel,
    sendMessage,
    stop,
    loadEarlierHistory,
    submitExtensionDialog,
    cancelExtensionDialog,
    renameSession,
    selectModel,
    selectEffort,
  };
}

export default useTau;
