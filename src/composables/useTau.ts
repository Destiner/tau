import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { computed, nextTick, watch, type WatchStopHandle } from 'vue';

import { errorCopy } from '../lib/error-copy';
import type { PiBridgeEvent } from '../lib/pi/bridge';
import type { ThinkingLevel } from '../lib/pi/model-scope';
import { frontendOwnership, OwnershipClaimError } from '../lib/pi/ownership';
import {
  applySessionName,
  appendOptimisticPrompt,
  cancelExtensionDialog as sendExtensionDialogCancellation,
  cancelPendingPrompt,
  clearAbortWatch,
  clearPendingQueue,
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
  requestSessionNameRefresh,
  releaseIdleRuntimes,
  releaseRuntime,
  removeEmptyActivePhantom,
  removeProjectUiState,
  removeRegisteredEphemeralSession,
  rpc,
  sendPhantomMessage,
  startController,
  stopControllerProcess,
  submitQueuedMessage,
  submitExtensionDialog as sendExtensionDialogResponse,
  watchAbort,
  watchSettingRequest,
} from '../lib/pi/runtime';
import { hydrateTranscript } from '../lib/pi/transcript';
import {
  invokeTraced,
  startActionSpan,
  startActionMilestones,
  type TelemetryScope,
} from '../lib/telemetry';

import { advanceArchiveRevision, archiveRevision } from './archive-revision';
import {
  activeController,
  activeExtensionDialog,
  activeFeedback,
  activeProject,
  projectActionsDisabled,
  applyRemoteDirectoryListing,
  canArchiveSession,
  canCompose,
  canDraft,
  canQueue,
  activeQueue,
  queueBusy,
  queueFeedback,
  queueFailedDrafts,
  canReconnectRemote,
  canRenameSession,
  clearActiveSession,
  acknowledgeFeedback,
  clearControllerFeedback,
  clearRemoteDirectoryBrowser,
  clearRemoteRetry,
  commands,
  compacting,
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
  inProgressSessionCount,
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
  reopenRemoteFeedback,
  retryPresentation,
  runtimeAvailable,
  sessionLastUserMessageAt,
  sessionSortAt,
  sessionIndicator,
  sessionTooltipStatus,
  expandedRelativeTime,
  sessionLastActive,
  sessionLoading,
  sessionTitle,
  tooltipTitleMarkdown,
  activeSession,
  setActiveSessionView,
  setControllerError,
  setControllerLifecycle,
  setWorkspaceError,
  settingsDisabled,
  state,
  stopping,
  streaming,
  type ProjectSummary,
  type RemoteDirectoryChoice,
  type RemoteDirectoryListing,
  type SessionController,
  type SessionSummary,
  type WorkspaceSnapshot,
} from './state';

interface InitializationOperation {
  lifecycle: number;
  promise: Promise<void>;
}

let unlisten: UnlistenFn | undefined;
let initialization: InitializationOperation | undefined;
let lifecycle = 0;

// Serialize workspace writes without making navigation wait for disk I/O.
let workspaceWrite: Promise<void> = Promise.resolve();
const foldIntent = new WeakMap<
  ProjectSummary,
  { revision: number; confirmed: boolean }
>();
const archiveIntent = new WeakMap<SessionSummary, { confirmed: boolean }>();
let orderRevision = 0;
let currentSelectionTracker: (() => void) | undefined;

function cancelSavedPreview(): void {
  void invoke('cancel_saved_transcript').catch(() => undefined);
}

function trackSelection(
  controller: SessionController,
  milestones: ReturnType<typeof startActionMilestones>,
  projectPath: string,
  sessionId: string,
): void {
  currentSelectionTracker?.();
  let stop: WatchStopHandle = () => undefined;
  let readable = false;
  const current = (): boolean =>
    state.activeControllerKey === controller.key &&
    state.activeProjectPath === projectPath &&
    state.activeSessionId === sessionId &&
    !controller.disposed;
  const cancel = (): void => {
    stop();
    milestones.cancel();
    if (currentSelectionTracker === cancel) currentSelectionTracker = undefined;
  };
  currentSelectionTracker = cancel;
  stop = watch(
    () =>
      [
        controller.ready,
        controller.savedContentLoaded,
        controller.messagesLoaded,
        controller.messages.length,
      ] as const,
    () => {
      if (!current()) {
        cancel();
        return;
      }
      if (controller.ready) milestones.mark('ready');
      if (
        !readable &&
        (controller.savedContentLoaded ||
          controller.messagesLoaded ||
          controller.messages.length > 0)
      ) {
        readable = true;
        milestones.mark(
          controller.messagesLoaded
            ? 'readable_rpc'
            : controller.savedContentLoaded
              ? 'readable_saved'
              : 'readable_memory',
        );
      }
      if (controller.messagesLoaded) milestones.mark('hydrated');
    },
    { immediate: true, flush: 'sync' },
  );
  void nextTick(() => {
    if (current()) milestones.afterRender();
    else cancel();
  });
}

function trackSelectionWrite(
  write: Promise<boolean>,
  milestones: ReturnType<typeof startActionMilestones>,
): void {
  void write.then((success) =>
    milestones.mark(success ? 'persisted' : 'persistence_failed'),
  );
}

function mergeImportedProject(project: ProjectSummary): void {
  const workspace = state.workspace;
  if (!workspace) return;
  const index = workspace.projects.findIndex(
    (item) => item.path === project.path,
  );
  if (index < 0) workspace.projects.push(project);
  else workspace.projects.splice(index, 1, project);
  if (project.selected) {
    workspace.activeProjectPath = project.path;
    for (const item of workspace.projects)
      item.selected = item.path === project.path;
  }
}

function writeWorkspace(
  command:
    | 'set_project_collapsed'
    | 'reorder_projects'
    | 'remove_project'
    | 'archive_session'
    | 'unarchive_session',
  args: Record<string, unknown>,
): Promise<void> {
  const write = workspaceWrite
    .catch(() => undefined)
    .then(() => invokeTraced<void>(command, args));
  workspaceWrite = write.catch(() => undefined);
  return write;
}

interface SavedTranscript {
  messages: unknown[] | null;
}

const previewNavigation = new WeakMap<SessionController, number>();
function readSavedSession(
  project: ProjectSummary,
  session: SessionSummary,
  controller: SessionController,
): void {
  if (!session.path || controller.messagesLoaded || controller.messages.length)
    return;
  const navigation = lifecycle;
  const activation = (previewNavigation.get(controller) ?? 0) + 1;
  previewNavigation.set(controller, activation);
  const generation = controller.generation;
  const streamSequence = controller.streamSequence;
  const hydrationSequence = controller.messagesHydrationSequence;
  void invoke<SavedTranscript>('read_saved_transcript', {
    projectPath: project.path,
    sessionId: session.id,
    sessionPath: session.path,
  })
    .then(({ messages }) => {
      if (
        !Array.isArray(messages) ||
        navigation !== lifecycle ||
        previewNavigation.get(controller) !== activation ||
        controller.disposed ||
        (generation !== 0 && controller.generation !== generation) ||
        controller.streamSequence !== streamSequence ||
        controller.messagesHydrationSequence !== hydrationSequence ||
        controller.messagesLoaded ||
        controller.messages.length > 0 ||
        controller.streaming ||
        state.activeControllerKey !== controller.key ||
        state.activeSessionId !== session.id ||
        state.activeProjectPath !== project.path
      )
        return;
      controller.messages = hydrateTranscript(messages, controller.messages);
      controller.savedContentLoaded = true;
    })
    .catch(() => undefined);
}

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

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function useTau() {
  function initialize(): Promise<void> {
    if (initialization) {
      initialization.lifecycle = lifecycle;
      state.initializing = true;
      return initialization.promise;
    }
    state.initializing = true;
    state.ownershipFailure = null;
    const operation: InitializationOperation = {
      lifecycle,
      promise: Promise.resolve(),
    };
    operation.promise = initializeOnce(operation)
      .catch(() => {
        if (operation.lifecycle === lifecycle) {
          setWorkspaceError(errorCopy.piOwnership);
          state.ownershipFailure = 'retryable';
        }
      })
      .finally(() => {
        if (initialization === operation) initialization = undefined;
        if (operation.lifecycle === lifecycle) {
          state.initializing = false;
        } else {
          unlisten?.();
          unlisten = undefined;
        }
      });
    initialization = operation;
    return operation.promise;
  }

  async function initializeOnce(
    operation: InitializationOperation,
  ): Promise<void> {
    if (!unlisten) {
      const nextUnlisten = await listen<PiBridgeEvent>(
        'pi-event',
        ({ payload }) => {
          void handleBridgeEvent(payload).catch(() => {
            const controller = controllerByRuntimeId(payload.runtimeId);
            if (!controller || controller.generation !== payload.generation) {
              return;
            }
            setControllerLifecycle(
              controller,
              { syncing: false },
              'bridge_event_failed',
            );
            setControllerError(controller, errorCopy.bridgeEvent);
            releaseRuntime(controller);
          });
        },
      );
      if (operation.lifecycle !== lifecycle) {
        nextUnlisten();
        return;
      }
      unlisten = nextUnlisten;
    }

    try {
      await frontendOwnership.claim();
    } catch (error) {
      if (operation.lifecycle === lifecycle) {
        const conflict =
          error instanceof OwnershipClaimError && error.kind === 'conflict';
        state.ownershipFailure = conflict ? 'conflict' : 'retryable';
        setWorkspaceError(
          conflict ? errorCopy.piOwnershipConflict : errorCopy.piOwnership,
        );
      }
      return;
    }
    if (operation.lifecycle !== lifecycle) return;

    try {
      const workspace = await invokeTraced<WorkspaceSnapshot>(
        'load_workspace',
        {},
      );
      if (operation.lifecycle !== lifecycle) return;
      state.workspace = workspace;
      state.activeProjectPath = state.workspace.activeProjectPath;
      const selectedProject = state.workspace.projects.find(
        (project) => project.selected,
      );
      const selectedSession = selectedProject?.sessions.find(
        (session) => session.selected,
      );
      if (
        selectedProject &&
        !selectedSession &&
        projectSessions(selectedProject).length === 0
      ) {
        const controller = await newSession(selectedProject, true);
        if (operation.lifecycle !== lifecycle && controller) {
          await stopControllerProcess(controller, undefined, false);
        }
      } else if (selectedProject && selectedSession) {
        const controller = ensureController(selectedProject, selectedSession);
        setActiveSessionView(selectedProject, selectedSession, controller);
        const needsLocalRuntime = !selectedProject.connectionString;
        if (needsLocalRuntime && !state.workspace.piPath) {
          setControllerError(controller, errorCopy.piNotFound);
          return;
        }
        readSavedSession(selectedProject, selectedSession, controller);
        await startController(
          controller,
          selectedProject,
          selectedSession.path,
        );
        if (operation.lifecycle !== lifecycle) {
          await stopControllerProcess(controller, undefined, false);
        }
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
    currentSelectionTracker?.();
    cancelSavedPreview();
    lifecycle += 1;
    if (!initialization) state.initializing = false;
    if (!initialization) {
      unlisten?.();
      unlisten = undefined;
    }
    for (const controller of state.controllers) {
      controller.retry = undefined;
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
      mergeImportedProject(
        await invokeTraced<ProjectSummary>('import_project', {
          path: selection,
        }),
      );
      if (state.workspace && !state.workspace.piPath) {
        state.workspace.piPath = (
          await invokeTraced<WorkspaceSnapshot>('load_workspace', {})
        ).piPath;
      }
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace?.activeProjectPath ?? '';
      }
      if (!state.activeSessionId) {
        const selectedProject = state.workspace?.projects.find(
          (project) => project.selected,
        );
        if (selectedProject) {
          const selectedSession = selectedProject.sessions.find(
            (session) => session.selected,
          );
          if (selectedSession) {
            await selectSession(selectedProject, selectedSession);
          } else if (projectSessions(selectedProject).length === 0) {
            await newSession(selectedProject);
          }
        }
      }
    } catch {
      setWorkspaceError(errorCopy.importProject);
    }
  }

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

  async function toggleProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    const previous = foldIntent.get(project);
    const intent = {
      revision: (previous?.revision ?? 0) + 1,
      confirmed: previous?.confirmed ?? project.collapsed,
    };
    foldIntent.set(project, intent);
    const collapsed = !project.collapsed;
    project.collapsed = collapsed;
    try {
      await writeWorkspace('set_project_collapsed', {
        path: project.path,
        collapsed,
      });
      intent.confirmed = collapsed;
      if (foldIntent.get(project)?.revision !== intent.revision)
        foldIntent.get(project)!.confirmed = collapsed;
    } catch {
      if (foldIntent.get(project)?.revision === intent.revision)
        project.collapsed = foldIntent.get(project)!.confirmed;
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
      projectActionsDisabled.value ||
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
    const revision = ++orderRevision;

    try {
      await writeWorkspace('reorder_projects', {
        projectPaths: projects.map((candidate) => candidate.path),
      });
    } catch {
      if (orderRevision === revision && state.workspace)
        state.workspace.projects = workspace.projects;
      setWorkspaceError(errorCopy.projectOrder);
    }
  }

  async function removeProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    state.removingProjectPaths.push(project.path);
    const projectControllers = state.controllers.filter(
      (controller) => controller.projectPath === project.path,
    );

    try {
      const removingActiveView = project.path === state.activeProjectPath;
      await writeWorkspace('remove_project', { path: project.path });
      const currentWorkspace = state.workspace;
      if (currentWorkspace) {
        currentWorkspace.projects = currentWorkspace.projects.filter(
          (candidate) => candidate.path !== project.path,
        );
        if (currentWorkspace.activeProjectPath === project.path)
          currentWorkspace.activeProjectPath = '';
      }
      if (removingActiveView && state.activeProjectPath === project.path) {
        clearActiveSession();
      }
      if (!state.activeProjectPath) {
        state.activeProjectPath = currentWorkspace?.activeProjectPath ?? '';
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
    const controller = controllerForSession(project.path, session.id);
    clearControllerFeedback(controller, errorCopy.archiveSession);
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

    const intent = archiveIntent.get(session) ?? {
      confirmed: session.archived,
    };
    archiveIntent.set(session, intent);
    const revision = advanceArchiveRevision(project.path, session.id);
    session.archived = true;
    session.selected = false;
    if (controller) removeRegisteredEphemeralSession(controller);
    if (discardedViewWasSelected) {
      const nextSession = projectSessions(project)[0];
      if (nextSession) void selectSession(project, nextSession);
      else void newSession(project);
    }
    try {
      await writeWorkspace('archive_session', {
        projectPath: project.path,
        sessionId: session.id,
      });
      intent.confirmed = true;
      const archivedViewStillSelected =
        state.activeProjectPath === project.path &&
        state.activeSessionId === session.id;
      if (!archivedViewStillSelected) releaseRuntime(controller);
    } catch {
      if (archiveRevision(project.path, session.id) === revision)
        session.archived = intent.confirmed;
      const errorController = controller ?? ensureController(project, session);
      setControllerError(errorController, errorCopy.archiveSession);
      if (!isSessionSelected(project, session)) errorController.unread = true;
    }
  }

  async function unarchiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    const intent = archiveIntent.get(session) ?? {
      confirmed: session.archived,
    };
    archiveIntent.set(session, intent);
    const revision = advanceArchiveRevision(project.path, session.id);
    session.archived = false;
    try {
      await writeWorkspace('unarchive_session', {
        projectPath: project.path,
        sessionId: session.id,
      });
      intent.confirmed = false;
    } catch {
      if (archiveRevision(project.path, session.id) === revision)
        session.archived = intent.confirmed;
      setWorkspaceError(errorCopy.restoreSession);
    }
  }

  async function newSession(
    project: ProjectSummary,
    bootstrap = false,
  ): Promise<SessionController | undefined> {
    if (!bootstrap && projectActionsDisabled.value) return;
    const milestones = startActionMilestones('session.new');
    const actionSpan = milestones.span;
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
      cancelSavedPreview();
      trackSelection(controller, milestones, project.path, session.id);
      trackSelectionWrite(
        persistProjectSelection(project.path, controller, actionSpan.context),
        milestones,
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
      return controller;
    } finally {
      actionSpan.end();
    }
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    const milestones = startActionMilestones(
      'session.select',
      controllerTelemetryScope(
        controllerForSession(project.path, session.id),
        session.id,
      ),
    );
    const actionSpan = milestones.span;
    try {
      const alreadySelected =
        project.path === state.activeProjectPath &&
        session.id === state.activeSessionId;
      if (alreadySelected) {
        const controller = activeController.value;
        if (controller) {
          controller.unread = false;
          if (controller.remoteDisconnected) reopenRemoteFeedback(controller);
          if (
            !controller.phantom &&
            !controller.ready &&
            !controller.starting &&
            !controller.remoteDisconnected
          ) {
            readSavedSession(project, session, controller);
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
      if (
        !session.path ||
        controller.messagesLoaded ||
        controller.messages.length
      )
        cancelSavedPreview();
      if (controller.remoteDisconnected) reopenRemoteFeedback(controller);
      controller.unread = false;
      trackSelection(controller, milestones, project.path, session.id);
      trackSelectionWrite(
        persistProjectSelection(project.path, controller, actionSpan.context),
        milestones,
      );
      releaseIdleRuntimes();

      if (controller.ready && !controller.messagesLoaded)
        readSavedSession(project, session, controller);
      if (
        controller.phantom ||
        controller.ready ||
        controller.starting ||
        controller.remoteDisconnected
      )
        return;
      readSavedSession(project, session, controller);
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

  const extensionCommandDraft = computed(() => {
    const controller = activeController.value;
    return Boolean(
      controller && invokesExtensionCommand(controller, draft.value.trim()),
    );
  });

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
    state,
    activeProject,
    projectActionsDisabled,
    activeController,
    messages,
    draft,
    retryPresentation,
    activeFeedback,
    acknowledgeFeedback,
    streaming,
    compacting,
    stopping,
    promptSubmitting,
    inProgressSessionCount,
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
    canQueue,
    activeQueue,
    queueBusy,
    queueFeedback,
    queueFailedDrafts,
    extensionCommandDraft,
    dismissQueueFeedback,
    canReconnectRemote,
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
    sessionTooltipStatus,
    expandedRelativeTime,
    isSessionUnread,
    markSessionUnread,
    markSessionRead,
    projectIndicator,
    indicatorLabel,
    sendMessage,
    clearQueue,
    recoverQueueDraft,
    reconnectRemoteSession,
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
