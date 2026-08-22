import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

import type { PiBridgeEvent } from '../lib/pi/bridge';
import type { ThinkingLevel } from '../lib/pi/model-scope';
import {
  applySessionName,
  appendOptimisticPrompt,
  cancelExtensionDialog as sendExtensionDialogCancellation,
  cancelPendingPrompt,
  clearAbortWatch,
  clearExtensionUiState,
  clearSessionReplacementWatch,
  discardUnregisteredEphemeralSession,
  handleBridgeEvent,
  invokesExtensionCommand,
  persistExpandedProject,
  persistProjectSelection,
  registerConnectedSession,
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
  applyRemoteDirectoryListing,
  canArchiveSession,
  canCompose,
  canDraft,
  canRenameSession,
  clearActiveSession,
  clearRemoteDirectoryBrowser,
  clearRemoteRetry,
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
  errorMessage,
  indicatorLabel,
  inheritControllerSettings,
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
  archivedSessionEntries,
  relativeTimestamp,
  runtimeAvailable,
  sessionLastUserMessageAt,
  sessionIndicator,
  sessionLastActive,
  sessionLoading,
  sessionTitle,
  setActiveError,
  setActiveSessionView,
  setControllerError,
  setControllerLifecycle,
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
        void handleBridgeEvent(payload).catch((error) => {
          const controller = controllerByRuntimeId(payload.runtimeId);
          if (!controller) return;
          setControllerLifecycle(
            controller,
            { syncing: false },
            'bridge_event_failed',
          );
          setControllerError(controller, error);
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
          controller.status =
            'Pi was not found. Install pi or set TAU_PI_PATH, then restart Tau.';
          return;
        }
        await startController(
          controller,
          selectedProject,
          selectedSession.path,
        );
      }
    } catch (error) {
      const controller = activeController.value;
      if (controller) {
        setControllerLifecycle(
          controller,
          { starting: false },
          'workspace_load_failed',
        );
        setControllerError(controller, error);
      }
    }
  }

  function dispose(): void {
    unlisten?.();
    unlisten = undefined;
    for (const controller of state.controllers) {
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
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
    try {
      const selection = await open({
        directory: true,
        multiple: false,
        title: 'Choose a project folder',
      });
      if (!selection) return;
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'import_project',
        { path: selection },
      );
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
    } catch (error) {
      setActiveError(error);
    }
  }

  function openRemoteProjectDialog(): void {
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
      cancelPendingPrompt(
        retryController,
        'The remote connection was cancelled.',
      );
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
    } catch (error) {
      state.remoteConnectionError = errorMessage(error);
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
    } catch (error) {
      state.remoteConnectionError = errorMessage(error);
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
      state.remoteConnectionError =
        'The remote session is no longer available.';
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
    } catch (error) {
      state.remoteConnecting = false;
      state.remoteConnectionError = errorMessage(error);
    }
  }

  async function toggleProject(project: ProjectSummary): Promise<void> {
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'set_project_collapsed',
        { path: project.path, collapsed: !project.collapsed },
      );
    } catch (error) {
      setActiveError(error);
    }
  }

  async function reorderProjects(
    fromIndex: number,
    toIndex: number,
  ): Promise<void> {
    const workspace = state.workspace;
    if (
      !workspace ||
      fromIndex < 0 ||
      fromIndex >= workspace.projects.length ||
      toIndex < 0 ||
      toIndex >= workspace.projects.length ||
      fromIndex === toIndex
    ) {
      return;
    }

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
    } catch (error) {
      state.workspace = workspace;
      setActiveError(error);
    }
  }

  async function removeProject(project: ProjectSummary): Promise<void> {
    const projectControllers = state.controllers.filter(
      (controller) => controller.projectPath === project.path,
    );
    for (const controller of projectControllers) controller.disposed = true;
    await Promise.all(
      projectControllers.map((controller) => stopControllerProcess(controller)),
    );

    try {
      const removingActiveView = project.path === state.activeProjectPath;
      removeProjectUiState(project.path);
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'remove_project',
        { path: project.path },
      );
      if (removingActiveView) clearActiveSession();
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
    } catch (error) {
      setActiveError(error);
    }
  }

  async function archiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (!canArchiveSession(project, session)) return;
    const controller = controllerForSession(project.path, session.id);
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
    } catch (error) {
      setActiveError(error);
    }
  }

  async function unarchiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    try {
      state.workspace = await invokeTraced<WorkspaceSnapshot>(
        'unarchive_session',
        { projectPath: project.path, sessionId: session.id },
      );
      // The record is reachable again, but nothing selects it here: the
      // archived list is a review surface, not a session switcher.
    } catch (error) {
      setActiveError(error);
    }
  }

  async function newSession(project: ProjectSummary): Promise<void> {
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
      controller.stopping
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

      controller.draft = '';
      setControllerLifecycle(
        controller,
        { working: true },
        'message_send',
        actionSpan.context,
      );
      if (!command) {
        appendOptimisticPrompt(
          controller,
          message,
          `optimistic-user-${Date.now()}`,
        );
      }
      controller.status = '';
      try {
        const requestId = nextRequestId('prompt');
        if (command) controller.commandPromptRequestId = requestId;
        await rpc(
          controller,
          { id: requestId, type: 'prompt', message },
          actionSpan.context,
        );
        if (!command) {
          await registerConnectedSession(controller, actionSpan.context);
        }
      } catch (error) {
        setControllerLifecycle(
          controller,
          { working: false },
          'message_send_failed',
          actionSpan.context,
        );
        controller.commandPromptRequestId = '';
        setControllerError(controller, error);
      }
    } finally {
      actionSpan.end();
    }
  }

  async function stop(): Promise<void> {
    const controller = activeController.value;
    if (!controller?.streaming || controller.stopping) return;
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
      } catch (error) {
        clearAbortWatch(controller);
        setControllerLifecycle(
          controller,
          { stopping: false },
          'stop_failed',
          actionSpan.context,
        );
        setControllerError(controller, error);
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
      try {
        await rpc(
          controller,
          {
            id: nextRequestId('set-model'),
            type: 'set_model',
            provider: model.provider,
            modelId: model.id,
          },
          actionSpan.context,
        );
      } catch (error) {
        setControllerError(controller, error);
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
      } catch (error) {
        const pending = controller.pendingSessionRename;
        if (pending?.requestId === requestId) {
          applySessionName(
            controller,
            pending.previousName,
            pending.previousTitle,
          );
          controller.pendingSessionRename = undefined;
        }
        setControllerError(controller, error);
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
      controller.pendingEffort = level;
      try {
        await rpc(
          controller,
          {
            id: nextRequestId('set-effort'),
            type: 'set_thinking_level',
            level,
          },
          actionSpan.context,
        );
      } catch (error) {
        controller.pendingEffort = '';
        setControllerError(controller, error);
      }
    } finally {
      actionSpan.end();
    }
  }

  async function submitExtensionDialog(value: string | boolean): Promise<void> {
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
    activeController,
    messages,
    draft,
    status,
    streaming,
    stopping,
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
    projectSessions,
    archivedSessionEntries,
    sessionLastActive,
    sessionLastUserMessageAt,
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
    submitExtensionDialog,
    cancelExtensionDialog,
    renameSession,
    selectModel,
    selectEffort,
  };
}

export default useTau;
