import { computed, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  appendLocalErrors,
  asRecord,
  hydrateTranscript,
  messageFailure,
  stringValue,
  toolArgumentsText,
  toolResultText,
  toolSummary,
} from "../lib/pi/transcript";
import { describePiError } from "../lib/pi/error";
import { scopeModels } from "../lib/pi/model-scope";
import type { PiBridgeEvent } from "../lib/pi/bridge";
import type {
  CommandOption,
  ExtensionDialog,
  ExtensionDialogMethod,
  ExtensionNotification,
  ExtensionNotificationType,
  ModelOption,
  ProjectSummary,
  RemoteDirectoryEntry,
  RemoteDirectoryListing,
  SessionSummary,
  ThinkingLevel,
  TranscriptEntry,
  WorkspaceSnapshot,
} from "../types";

const effortLabels: Record<ThinkingLevel, string> = {
  off: "Off",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

export type SessionIndicator = "new" | "draft" | "working" | "";

const indicatorLabels: Record<Exclude<SessionIndicator, "">, string> = {
  new: "Unread",
  draft: "Unsent draft",
  working: "Working",
};

interface EphemeralSession extends SessionSummary {
  projectPath: string;
  controllerKey: string;
  createdAt: number;
  phantom: boolean;
}

interface PendingPrompt {
  message: string;
  optimisticId: string;
  command: boolean;
  stateRequestId: string;
  messagesRequestId: string;
  selectedModelProvider: string;
  selectedModelId: string;
  selectedModelName: string;
  selectedEffort: ThinkingLevel;
  settingsRequestId: string;
  settingsStep: "" | "model" | "effort";
}

interface SessionController {
  key: string;
  runtimeId: string;
  projectPath: string;
  sessionId: string;
  sessionPath: string;
  sessionName: string;
  phantom: boolean;
  generation: number;
  ready: boolean;
  streaming: boolean;
  stopping: boolean;
  starting: boolean;
  working: boolean;
  unread: boolean;
  lastUserMessageAt: number;
  messages: TranscriptEntry[];
  /** Failures Pi reports as events only; its message list never carries them. */
  localErrors: string[];
  draft: string;
  status: string;
  currentModelProvider: string;
  currentModelId: string;
  currentModelName: string;
  currentEffort: ThinkingLevel;
  pendingEffort: ThinkingLevel | "";
  models: ModelOption[];
  modelScope: string[];
  efforts: ThinkingLevel[];
  commands: CommandOption[];
  commandsLoaded: boolean;
  pendingPrompt?: PendingPrompt;
  bootstrapStateRequestId: string;
  bootstrapSessionPath: string;
  runStateRequestId: string;
  startMessagesRequestId: string;
  commandPromptRequestId: string;
  commandSyncRequestId: string;
  replacementProbeRequestId: string;
  abortProbeRequestId: string;
  connectingRemote: boolean;
  syncing: boolean;
  lastActiveSequence: number;
  disposed: boolean;
  streamSequence: number;
}

interface RemoteRetry {
  controllerKey: string;
  projectPath: string;
  sessionPath?: string;
  preserveMessages: boolean;
}

type RemoteDialogMode = "add" | "retry";
type RemoteDialogStep = "connection" | "directory";
type RemoteDirectoryChoice = "back" | "select" | "forward";

const state = reactive({
  workspace: null as WorkspaceSnapshot | null,
  activeProjectPath: "",
  activeSessionId: "",
  activeSessionPath: "",
  activeControllerKey: "",
  workspaceStatus: "",
  controllers: [] as SessionController[],
  ephemeralSessions: [] as EphemeralSession[],
  extensionDialogs: [] as ExtensionDialog[],
  extensionNotifications: [] as ExtensionNotification[],
  remoteDialogOpen: false,
  remoteDialogMode: "add" as RemoteDialogMode,
  remoteDialogStep: "connection" as RemoteDialogStep,
  remoteConnectionString: "",
  remoteConnectionError: "",
  remoteConnecting: false,
  remoteDirectoryHost: "",
  remoteDirectoryRoot: "",
  remoteWorkingDirectory: "",
  remoteDirectoryHistory: [] as string[],
  remoteDirectories: [] as RemoteDirectoryEntry[],
  remoteDirectoryFilter: "",
  remoteDirectorySelectedIndex: 0,
  requestSequence: 0,
});

/**
 * Pi has no session-replacement event, and a `get_state` sent the moment a run
 * settles still reports the outgoing session because extensions swap sessions
 * from a deferred callback. These delays re-check session identity after the
 * moments a replacement can happen, so a spawned phase session is registered
 * without waiting for its first user message.
 */
const replacementProbeDelays = [150, 450, 1_000, 2_000, 3_200];

/**
 * Pi answers an abort only once the agent has gone idle, so a tool call that
 * ignores the abort signal withholds both the acknowledgement and the settle
 * event for as long as it keeps running. Tau re-reads Pi's own run state after
 * this grace period rather than leaving the session locked on a stop it cannot
 * confirm.
 */
const abortAcknowledgeDelay = 2_000;

/**
 * Stopping an idle runtime is not the neutral act it looks like. Pi hands a
 * session-opening context to extensions only inside a command handler or a
 * `withSession` callback, so an extension that drives a multi-session workflow
 * has to hold that context in the process it was given. The process is the
 * only place that state exists: reopening the session file restores the
 * transcript but not the handoff, and a workflow phase that spans a user turn
 * then completes with nothing left to open its next session.
 *
 * Idle hidden runtimes are therefore kept warm, and only the least recently
 * active ones past this limit are released. Runtimes that are still working
 * are never released, so the live total can exceed the limit while work runs.
 */
const idleRuntimeLimit = 6;

let unlisten: UnlistenFn | undefined;
let phantomSequence = 0;
let controllerSequence = 0;
let activitySequence = 0;
let remoteRetry: RemoteRetry | undefined;
const extensionDialogTimeouts = new Map<
  string,
  ReturnType<typeof setTimeout>
>();
const replacementProbeTimers = new Map<
  string,
  ReturnType<typeof setTimeout>[]
>();
const abortProbeTimers = new Map<string, ReturnType<typeof setTimeout>>();
const extensionNotificationTimeouts = new Map<
  string,
  ReturnType<typeof setTimeout>
>();

const emptyMessages: TranscriptEntry[] = [];
const emptyModels: ModelOption[] = [];
const emptyEfforts: ThinkingLevel[] = [];
const emptyCommands: CommandOption[] = [];

const activeProject = computed(() =>
  state.workspace?.projects.find(
    (project) => project.path === state.activeProjectPath,
  ),
);

const activeSession = computed(() =>
  activeProject.value
    ? projectSessions(activeProject.value).find(
        (session) => session.id === state.activeSessionId,
      )
    : undefined,
);

const activeController = computed(() =>
  state.controllers.find(
    (controller) => controller.key === state.activeControllerKey,
  ),
);

const messages = computed(
  () => activeController.value?.messages ?? emptyMessages,
);
const draft = computed({
  get: () => activeController.value?.draft ?? "",
  set: (value: string) => {
    const controller = activeController.value;
    if (!controller) return;
    controller.draft = value;
    const session = ephemeralSessionByController(controller.key);
    if (session?.phantom) session.title = draftTitle(value);
  },
});
const status = computed(
  () => activeController.value?.status || state.workspaceStatus,
);
const streaming = computed(() => activeController.value?.streaming === true);
const stopping = computed(() => activeController.value?.stopping === true);
const models = computed(() => {
  const controller = activeController.value;
  if (!controller) return emptyModels;
  return scopeModels(controller.models, controller.modelScope);
});
const efforts = computed(() => activeController.value?.efforts ?? emptyEfforts);
const commands = computed(
  () => activeController.value?.commands ?? emptyCommands,
);
const activeExtensionDialog = computed(() => {
  const controller = activeController.value;
  if (!controller) return undefined;
  return state.extensionDialogs.find(
    (dialog) => dialog.controllerKey === controller.key,
  );
});
const extensionNotifications = computed(() => state.extensionNotifications);
const currentModelProvider = computed(
  () => activeController.value?.currentModelProvider ?? "",
);
const currentModelId = computed(
  () => activeController.value?.currentModelId ?? "",
);
const currentEffort = computed(
  () => activeController.value?.currentEffort ?? "off",
);

const canDraft = computed(() =>
  Boolean(activeProject.value && activeSession.value && activeController.value),
);

/**
 * A saved session has nothing to show until its runtime hydrates the
 * transcript, so the pane reports loading instead of rendering the empty
 * session composer over a session that already holds messages.
 */
const sessionLoading = computed(() => {
  const controller = activeController.value;
  if (!controller || controller.phantom) return false;
  if (controller.messages.length > 0 || controller.status) return false;
  return !controller.ready || controller.starting || controller.syncing;
});

const canCompose = computed(() => {
  const controller = activeController.value;
  if (!activeProject.value || !activeSession.value || !controller) return false;
  if (controller.starting) return false;
  return controller.phantom
    ? runtimeAvailable(activeProject.value)
    : controller.ready;
});

const sessionTitle = computed(() => {
  const controller = activeController.value;
  return (
    controller?.sessionName ||
    activeSession.value?.title ||
    firstUserMessage(controller) ||
    "New session"
  );
});

const currentModelLabel = computed(() => {
  const controller = activeController.value;
  return controller?.currentModelName || controller?.currentModelId || "Model";
});

const currentEffortLabel = computed(
  () => effortLabels[activeController.value?.currentEffort ?? "off"],
);

const settingsDisabled = computed(() => {
  const controller = activeController.value;
  return (
    !controller ||
    (!controller.ready && !controller.phantom) ||
    controller.streaming ||
    controller.stopping ||
    controller.starting
  );
});

/**
 * Pi keeps the name inside the session it is running, so renaming needs a live
 * runtime and a session Pi has already opened. An unsent session shows a
 * preview of its draft instead of a name, so it has nothing to rename yet.
 */
const canRenameSession = computed(() => {
  const controller = activeController.value;
  return Boolean(controller && !controller.phantom && controller.ready);
});

export function useTau() {
  async function initialize() {
    if (!unlisten) {
      unlisten = await listen<PiBridgeEvent>("pi-event", ({ payload }) => {
        void handleBridgeEvent(payload).catch((error) => {
          const controller = controllerByRuntimeId(payload.runtimeId);
          if (!controller) return;
          controller.syncing = false;
          setControllerError(controller, error);
          releaseRuntime(controller);
        });
      });
    }
    try {
      state.workspace = await invoke<WorkspaceSnapshot>("load_workspace");
      state.workspaceStatus = "";
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
            "Pi was not found. Install pi or set TAU_PI_PATH, then restart Tau.";
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
        controller.starting = false;
        setControllerError(controller, error);
      }
    }
  }

  function dispose() {
    unlisten?.();
    unlisten = undefined;
    for (const controller of state.controllers) {
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
    }
    clearExtensionUiState();
  }

  async function addLocalProject() {
    try {
      const selection = await open({
        directory: true,
        multiple: false,
        title: "Choose a project folder",
      });
      if (!selection) return;
      state.workspace = await invoke<WorkspaceSnapshot>("import_project", {
        path: selection,
      });
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
    } catch (error) {
      setActiveError(error);
    }
  }

  function openRemoteProjectDialog() {
    remoteRetry = undefined;
    state.remoteDialogMode = "add";
    state.remoteDialogStep = "connection";
    state.remoteConnectionString = "";
    state.remoteConnectionError = "";
    state.remoteConnecting = false;
    clearRemoteDirectoryBrowser();
    state.remoteDialogOpen = true;
  }

  function closeRemoteProjectDialog() {
    if (state.remoteConnecting) return;
    const retryController = remoteRetry
      ? controllerByKey(remoteRetry.controllerKey)
      : undefined;
    if (state.remoteDialogMode === "retry" && retryController?.pendingPrompt) {
      cancelPendingPrompt(
        retryController,
        "The remote connection was cancelled.",
      );
    }
    remoteRetry = undefined;
    state.remoteDialogOpen = false;
    state.remoteConnectionError = "";
    clearRemoteDirectoryBrowser();
  }

  async function submitRemoteConnection() {
    if (state.remoteConnecting) return;
    if (state.remoteDialogMode === "retry") {
      await retryRemoteConnection();
      return;
    }

    const connectionString = state.remoteConnectionString.trim();
    if (!connectionString) {
      state.remoteConnectionError = "Enter an SSH connection string.";
      return;
    }
    state.remoteConnecting = true;
    state.remoteConnectionError = "";
    try {
      const listing = await invoke<RemoteDirectoryListing>(
        "probe_remote_project",
        { connectionString },
      );
      applyRemoteDirectoryListing(listing, true);
      state.remoteConnectionString = listing.connectionString;
      state.remoteDialogStep = "directory";
    } catch (error) {
      state.remoteConnectionError = errorMessage(error);
    } finally {
      state.remoteConnecting = false;
    }
  }

  async function chooseRemoteDirectory(
    path: string,
    choice: RemoteDirectoryChoice,
  ) {
    if (state.remoteConnecting || state.remoteDialogStep !== "directory") {
      return;
    }
    state.remoteConnecting = true;
    state.remoteConnectionError = "";
    try {
      if (choice === "select") {
        state.workspace = await invoke<WorkspaceSnapshot>(
          "import_remote_project",
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
        const listing = await invoke<RemoteDirectoryListing>(
          "list_remote_directories",
          {
            connectionString: state.remoteConnectionString,
            workingDirectory: path,
          },
        );
        if (choice === "back") state.remoteDirectoryHistory.pop();
        else state.remoteDirectoryHistory.push(currentDirectory);
        applyRemoteDirectoryListing(listing);
      }
    } catch (error) {
      state.remoteConnectionError = errorMessage(error);
    } finally {
      state.remoteConnecting = false;
    }
  }

  async function retryRemoteConnection() {
    const retry = remoteRetry;
    const controller = retry ? controllerByKey(retry.controllerKey) : undefined;
    const project = state.workspace?.projects.find(
      (item) => item.path === retry?.projectPath,
    );
    if (!retry || !controller || !project?.connectionString) {
      state.remoteConnectionError =
        "The remote session is no longer available.";
      return;
    }

    state.remoteConnecting = true;
    state.remoteConnectionError = "";
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

  async function toggleProject(project: ProjectSummary) {
    try {
      state.workspace = await invoke<WorkspaceSnapshot>(
        "set_project_collapsed",
        {
          path: project.path,
          collapsed: !project.collapsed,
        },
      );
    } catch (error) {
      setActiveError(error);
    }
  }

  async function reorderProjects(fromIndex: number, toIndex: number) {
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
      state.workspace = await invoke<WorkspaceSnapshot>("reorder_projects", {
        projectPaths: projects.map((candidate) => candidate.path),
      });
    } catch (error) {
      state.workspace = workspace;
      setActiveError(error);
    }
  }

  async function removeProject(project: ProjectSummary) {
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
      state.workspace = await invoke<WorkspaceSnapshot>("remove_project", {
        path: project.path,
      });
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
  ) {
    if (!canArchiveSession(project, session)) return;
    const controller = controllerForSession(project.path, session.id);

    try {
      state.workspace = await invoke<WorkspaceSnapshot>("archive_session", {
        projectPath: project.path,
        sessionId: session.id,
      });
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

  async function newSession(project: ProjectSummary) {
    const previous = activeController.value;
    removeEmptyActivePhantom();
    const controllerKey = nextControllerKey();
    const session = createPhantomSession(project.path, controllerKey);
    const controller = createController(project, session, controllerKey);
    inheritControllerSettings(controller, previous);
    state.ephemeralSessions.push(session);
    if (project.collapsed) {
      project.collapsed = false;
      void persistExpandedProject(project.path);
    }
    setActiveSessionView(project, session, controller);
    controller.status = "";
    await persistProjectSelection(project.path, controller);
    releaseIdleRuntimes();
    if (
      runtimeAvailable(project) &&
      (!controller.currentModelId ||
        controller.models.length === 0 ||
        controller.efforts.length === 0 ||
        !controller.commandsLoaded)
    ) {
      await startController(controller, project);
    }
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ) {
    const alreadySelected =
      project.path === state.activeProjectPath &&
      session.id === state.activeSessionId;
    if (alreadySelected) {
      const controller = activeController.value;
      if (controller) {
        controller.unread = false;
        if (!controller.phantom && !controller.ready && !controller.starting) {
          await startController(controller, project, session.path);
        }
      }
      return;
    }

    removeEmptyActivePhantom();
    const controller = ensureController(project, session);
    setActiveSessionView(project, session, controller);
    controller.status = "";
    controller.unread = false;
    await persistProjectSelection(project.path, controller);
    releaseIdleRuntimes();

    if (controller.phantom || controller.ready || controller.starting) return;
    await startController(controller, project, session.path);
  }

  async function sendMessage() {
    const controller = activeController.value;
    const message = controller?.draft.trim() ?? "";
    if (
      !controller ||
      !message ||
      !canCompose.value ||
      controller.streaming ||
      controller.stopping
    ) {
      return;
    }

    const command = invokesExtensionCommand(controller, message);
    if (!command) markUserMessageSubmitted(controller);

    if (controller.phantom) {
      await sendPhantomMessage(controller, message, command);
      return;
    }

    controller.draft = "";
    controller.working = true;
    if (!command) {
      controller.messages.push({
        id: `optimistic-user-${Date.now()}`,
        kind: "user",
        text: message,
      });
    }
    controller.status = "";
    try {
      const requestId = nextRequestId("prompt");
      if (command) controller.commandPromptRequestId = requestId;
      await rpc(controller, { id: requestId, type: "prompt", message });
      if (!command) await registerConnectedSession(controller);
    } catch (error) {
      controller.working = false;
      controller.commandPromptRequestId = "";
      setControllerError(controller, error);
    }
  }

  async function stop() {
    const controller = activeController.value;
    if (!controller?.streaming || controller.stopping) return;
    controller.stopping = true;
    controller.status = "";
    watchAbort(controller);
    try {
      await rpc(controller, { id: nextRequestId("abort"), type: "abort" });
    } catch (error) {
      clearAbortWatch(controller);
      controller.stopping = false;
      setControllerError(controller, error);
    }
  }

  async function selectModel(value: string) {
    const controller = activeController.value;
    const model = controller?.models.find(
      (option) => `${option.provider}/${option.id}` === value,
    );
    if (!controller || !model || settingsDisabled.value) return;
    controller.status = "";
    if (controller.phantom) {
      controller.currentModelProvider = model.provider;
      controller.currentModelId = model.id;
      controller.currentModelName = model.name;
      return;
    }
    try {
      await rpc(controller, {
        id: nextRequestId("set-model"),
        type: "set_model",
        provider: model.provider,
        modelId: model.id,
      });
    } catch (error) {
      setControllerError(controller, error);
    }
  }

  async function renameSession(name: string) {
    const controller = activeController.value;
    if (!controller || !canRenameSession.value) return;
    const next = normalizeSessionName(name);
    if (!next || next === controller.sessionName) return;
    controller.status = "";
    applySessionName(controller, next);
    try {
      await rpc(controller, {
        id: nextRequestId("session-name"),
        type: "set_session_name",
        name: next,
      });
    } catch (error) {
      setControllerError(controller, error);
    }
  }

  async function selectEffort(level: ThinkingLevel) {
    const controller = activeController.value;
    if (
      !controller ||
      !controller.efforts.includes(level) ||
      settingsDisabled.value
    ) {
      return;
    }
    controller.status = "";
    if (controller.phantom) {
      controller.currentEffort = level;
      return;
    }
    controller.pendingEffort = level;
    try {
      await rpc(controller, {
        id: nextRequestId("set-effort"),
        type: "set_thinking_level",
        level,
      });
    } catch (error) {
      controller.pendingEffort = "";
      setControllerError(controller, error);
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
    extensionNotifications,
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
    addLocalProject,
    openRemoteProjectDialog,
    closeRemoteProjectDialog,
    submitRemoteConnection,
    chooseRemoteDirectory,
    toggleProject,
    reorderProjects,
    removeProject,
    archiveSession,
    newSession,
    selectSession,
    canArchiveSession,
    projectSessions,
    sessionLastActive,
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
    dismissExtensionNotification,
    renameSession,
    selectModel,
    selectEffort,
  };
}

async function sendPhantomMessage(
  controller: SessionController,
  message: string,
  command: boolean,
) {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project || !controller.phantom) return;

  const optimisticId = `optimistic-user-${Date.now()}`;
  controller.pendingPrompt = {
    message,
    optimisticId,
    command,
    stateRequestId: "",
    messagesRequestId: "",
    selectedModelProvider: controller.currentModelProvider,
    selectedModelId: controller.currentModelId,
    selectedModelName: controller.currentModelName,
    selectedEffort: controller.currentEffort,
    settingsRequestId: "",
    settingsStep: "",
  };
  controller.draft = "";
  controller.working = true;
  if (!command) {
    controller.messages.push({ id: optimisticId, kind: "user", text: message });
  }
  controller.status = "";

  if (controller.ready && controller.generation) {
    controller.starting = true;
    const stateRequestId = nextRequestId("state");
    controller.pendingPrompt.stateRequestId = stateRequestId;
    await rpc(controller, { id: stateRequestId, type: "get_state" });
    return;
  }
  await startController(controller, project, undefined, true);
}

async function startController(
  controller: SessionController,
  project: ProjectSummary,
  sessionPath?: string,
  preserveMessages = false,
) {
  if (controller.starting || controller.disposed) return;
  controller.ready = false;
  controller.starting = true;
  controller.stopping = false;
  controller.connectingRemote = Boolean(project.connectionString);
  controller.bootstrapStateRequestId = "";
  controller.bootstrapSessionPath = sessionPath ?? "";
  controller.runStateRequestId = "";
  controller.startMessagesRequestId = "";
  controller.commandPromptRequestId = "";
  controller.commandSyncRequestId = "";
  controller.replacementProbeRequestId = "";
  controller.abortProbeRequestId = "";
  touchController(controller);
  clearSessionReplacementWatch(controller);
  clearAbortWatch(controller);
  if (!preserveMessages && controller.messages.length === 0) {
    controller.messages = [];
  }
  controller.models = [];
  controller.efforts = [];
  controller.commands = [];
  controller.commandsLoaded = false;
  controller.status = "";
  discardControllerDialogs(controller);
  void refreshModelScope(controller, project);

  try {
    if (project.connectionString) {
      controller.generation = await invoke<number>("start_pi_remote", {
        runtimeId: controller.runtimeId,
        connectionString: project.connectionString,
        workingDirectory: project.workingDirectory,
        sessionPath: sessionPath ?? null,
      });
    } else {
      controller.generation = await invoke<number>("start_pi", {
        runtimeId: controller.runtimeId,
        projectPath: project.workingDirectory,
        sessionPath: sessionPath ?? null,
      });
    }
    if (controller.disposed) {
      await invoke("stop_pi", { runtimeId: controller.runtimeId });
      return;
    }
    await requestBootstrap(controller);
  } catch (error) {
    controller.starting = false;
    controller.connectingRemote = false;
    controller.syncing = false;
    if (project.connectionString)
      presentRemoteConnectionError(controller, error);
    else setControllerError(controller, error);
    if (controller.pendingPrompt) cancelPendingPrompt(controller, error);
  }
}

/**
 * Pi resolves its model scope from settings at startup, and never reports it
 * over RPC, so Tau reads the same setting to offer the models Pi would list.
 * A failed read leaves the picker on the full catalogue.
 */
async function refreshModelScope(
  controller: SessionController,
  project: ProjectSummary,
) {
  try {
    const patterns = project.connectionString
      ? await invoke<string[]>("read_remote_model_scope", {
          connectionString: project.connectionString,
        })
      : await invoke<string[]>("read_model_scope");
    if (controller.disposed || !Array.isArray(patterns)) return;
    controller.modelScope = patterns.filter(
      (pattern) => typeof pattern === "string",
    );
  } catch {
    controller.modelScope = [];
  }
}

async function requestBootstrap(controller: SessionController) {
  await rpc(controller, {
    id: nextRequestId("models"),
    type: "get_available_models",
  });
  await rpc(controller, {
    id: nextRequestId("commands"),
    type: "get_commands",
  });
  const stateRequestId = nextRequestId("state");
  controller.bootstrapStateRequestId = stateRequestId;
  if (controller.pendingPrompt) {
    controller.pendingPrompt.stateRequestId = stateRequestId;
  }
  await rpc(controller, { id: stateRequestId, type: "get_state" });
}

async function rpc(
  controller: SessionController,
  request: Record<string, unknown>,
) {
  await invoke("send_pi", { runtimeId: controller.runtimeId, request });
}

async function submitExtensionDialog(value: string | boolean) {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  const response =
    dialog.method === "confirm" && typeof value === "boolean"
      ? {
          type: "extension_ui_response",
          id: dialog.requestId,
          confirmed: value,
        }
      : typeof value === "string"
        ? { type: "extension_ui_response", id: dialog.requestId, value }
        : {
            type: "extension_ui_response",
            id: dialog.requestId,
            cancelled: true,
          };
  await respondToExtensionDialog(dialog, response);
}

async function cancelExtensionDialog() {
  const dialog = activeExtensionDialog.value;
  if (!dialog) return;
  await respondToExtensionDialog(dialog, {
    type: "extension_ui_response",
    id: dialog.requestId,
    cancelled: true,
  });
}

async function respondToExtensionDialog(
  dialog: ExtensionDialog,
  response: Record<string, unknown>,
) {
  discardExtensionDialog(dialog.key);
  const controller = controllerByKey(dialog.controllerKey);
  if (
    !controller ||
    controller.disposed ||
    controller.runtimeId !== dialog.runtimeId ||
    controller.generation !== dialog.generation
  ) {
    return;
  }
  try {
    await rpc(controller, response);
    // An answered dialog is a common point for a workflow to open its next
    // session, and that swap is not reported by any event.
    watchSessionReplacement(controller);
  } catch (error) {
    setControllerError(controller, error);
  }
}

function handleExtensionUIRequest(
  controller: SessionController,
  request: Record<string, unknown>,
) {
  const requestId = stringValue(request.id);
  const method = stringValue(request.method);
  if (!requestId || !method) return;

  if (isExtensionDialogMethod(method)) {
    const origin = extensionRequestOrigin(controller);
    const timeout =
      typeof request.timeout === "number" &&
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
      draft: typeof request.prefill === "string" ? request.prefill : "",
      ...(typeof request.message === "string"
        ? { message: request.message }
        : {}),
      ...(Array.isArray(request.options)
        ? {
            options: request.options.filter(
              (option): option is string => typeof option === "string",
            ),
          }
        : {}),
      ...(typeof request.placeholder === "string"
        ? { placeholder: request.placeholder }
        : {}),
      ...(typeof request.prefill === "string"
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

  if (method === "notify" && typeof request.message === "string") {
    const origin = extensionRequestOrigin(controller);
    const type = extensionNotificationType(request.notifyType);
    const notification: ExtensionNotification = {
      key: extensionRequestKey(controller, requestId),
      message: request.message,
      type,
      projectName: origin.projectName,
      sessionName: origin.sessionName,
      ...(origin.workingDirectory
        ? { workingDirectory: origin.workingDirectory }
        : {}),
    };
    dismissExtensionNotification(notification.key);
    state.extensionNotifications.push(notification);
    while (state.extensionNotifications.length > 4) {
      const oldest = state.extensionNotifications[0];
      if (oldest) dismissExtensionNotification(oldest.key);
    }
    extensionNotificationTimeouts.set(
      notification.key,
      setTimeout(() => dismissExtensionNotification(notification.key), 8_000),
    );
    return;
  }

  if (method === "setStatus") return;

  if (method === "set_editor_text" && typeof request.text === "string") {
    controller.draft = request.text;
    const session = ephemeralSessionByController(controller.key);
    if (session?.phantom) session.title = draftTitle(request.text);
  }
}

function isExtensionDialogMethod(
  method: string,
): method is ExtensionDialogMethod {
  return (
    method === "select" ||
    method === "confirm" ||
    method === "input" ||
    method === "editor"
  );
}

function extensionDialogTitle(method: ExtensionDialogMethod): string {
  if (method === "select") return "Choose an option";
  if (method === "confirm") return "Confirm";
  if (method === "input") return "Enter a value";
  return "Edit text";
}

function extensionNotificationType(value: unknown): ExtensionNotificationType {
  return value === "warning" || value === "error" ? value : "info";
}

function extensionRequestOrigin(controller: SessionController) {
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
      "New session",
    // A remote project's files are on the other host, where nothing local can
    // open them, so its paths are left as text.
    ...(project && !project.connectionString
      ? { workingDirectory: project.workingDirectory }
      : {}),
  };
}

function extensionRequestKey(
  controller: SessionController,
  requestId: string,
): string {
  return `${controller.runtimeId}:${controller.generation}:${requestId}`;
}

function discardExtensionDialog(key: string) {
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
) {
  for (const dialog of [...state.extensionDialogs]) {
    if (
      dialog.controllerKey === controller.key &&
      (generation === undefined || dialog.generation === generation)
    ) {
      discardExtensionDialog(dialog.key);
    }
  }
}

function dismissExtensionNotification(key: string) {
  const timeout = extensionNotificationTimeouts.get(key);
  if (timeout) clearTimeout(timeout);
  extensionNotificationTimeouts.delete(key);
  const index = state.extensionNotifications.findIndex(
    (notification) => notification.key === key,
  );
  if (index >= 0) state.extensionNotifications.splice(index, 1);
}

function clearExtensionUiState() {
  for (const timeout of extensionDialogTimeouts.values()) clearTimeout(timeout);
  extensionDialogTimeouts.clear();
  state.extensionDialogs.splice(0);
  for (const timeout of extensionNotificationTimeouts.values()) {
    clearTimeout(timeout);
  }
  extensionNotificationTimeouts.clear();
  state.extensionNotifications.splice(0);
}

async function handleBridgeEvent(event: PiBridgeEvent) {
  const controller = controllerByRuntimeId(event.runtimeId);
  if (!controller) return;
  touchController(controller);
  if (event.kind === "started") {
    if (event.generation > controller.generation) {
      discardControllerDialogs(controller);
      controller.generation = event.generation;
    }
    return;
  }
  if (event.generation !== controller.generation) return;

  if (event.kind === "rpc" && event.line) {
    let value: unknown;
    try {
      value = JSON.parse(event.line);
    } catch {
      return;
    }
    await handleRpc(controller, value);
    return;
  }
  if (event.kind === "stderr") return;
  if (event.kind === "error") {
    const message = event.message || "The Pi connection failed.";
    if (controller.connectingRemote) {
      presentRemoteConnectionError(controller, message);
      return;
    }
    if (controller.pendingPrompt) cancelPendingPrompt(controller, message);
    else setControllerError(controller, message);
    return;
  }
  if (event.kind === "exited") {
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    discardControllerDialogs(controller, event.generation);
    const message =
      event.code === 0
        ? ""
        : event.message || "The Pi process stopped unexpectedly.";
    if (controller.connectingRemote) {
      presentRemoteConnectionError(
        controller,
        message || "The remote Pi process stopped before it was ready.",
      );
      return;
    }
    if (controller.pendingPrompt) {
      cancelPendingPrompt(controller, message || "The Pi process stopped.");
    }
    controller.ready = false;
    controller.streaming = false;
    controller.stopping = false;
    controller.starting = false;
    controller.working = false;
    controller.runStateRequestId = "";
    controller.generation = 0;
    controller.status = message;
  }
}

async function handleRpc(controller: SessionController, value: unknown) {
  const event = asRecord(value);
  if (!event) return;
  const type = stringValue(event.type);
  if (type === "extension_ui_request") {
    handleExtensionUIRequest(controller, event);
    return;
  }
  if (type === "response") {
    await handleResponse(controller, event);
    return;
  }
  if (type === "agent_start") {
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
    controller.localErrors = [];
    controller.streaming = true;
    controller.stopping = false;
    controller.working = true;
    controller.status = "";
    const stateRequestId = nextRequestId("run-state");
    controller.runStateRequestId = stateRequestId;
    await rpc(controller, { id: stateRequestId, type: "get_state" });
    return;
  }
  if (type === "message_update") {
    const delta = asRecord(event.assistantMessageEvent);
    const deltaType = stringValue(delta?.type);
    if (deltaType === "text_delta") {
      appendStream(controller, "assistant", stringValue(delta?.delta));
      if (!isControllerSelected(controller)) controller.unread = true;
    }
    if (deltaType === "thinking_delta") {
      appendStream(controller, "thinking", stringValue(delta?.delta));
    }
    return;
  }
  // A turn that failed is settled from Pi's messages soon after, but only once
  // the run ends, and a retry can hold that off for the length of its backoff.
  if (type === "message_end") {
    const failure = messageFailure(event.message);
    if (failure) pushError(controller, failure);
    return;
  }
  /**
   * A failed compaction is reported once and kept nowhere: Pi's message list
   * has no entry for it, so the row is held on the controller and re-appended
   * to every list hydrated until the next run begins.
   */
  if (type === "compaction_end") {
    const failure = stringValue(event.errorMessage);
    if (failure) {
      controller.localErrors.push(failure);
      pushError(controller, failure);
    }
    return;
  }
  if (type === "tool_execution_start") {
    const toolCallId = stringValue(event.toolCallId);
    const toolName = stringValue(event.toolName) || "tool";
    controller.messages.push({
      id: `stream-tool-${controller.streamSequence++}`,
      kind: "tool",
      text: toolSummary(event.args),
      toolCallId,
      toolName,
      toolRunning: true,
      toolErrored: false,
      toolArguments: toolArgumentsText(event.args),
    });
    return;
  }
  if (type === "tool_execution_end") {
    const toolCallId = stringValue(event.toolCallId);
    const tool = [...controller.messages]
      .reverse()
      .find(
        (entry) => entry.kind === "tool" && entry.toolCallId === toolCallId,
      );
    if (tool) {
      tool.toolRunning = false;
      tool.toolErrored = event.isError === true;
      tool.toolResult = toolResultText(asRecord(event.result)?.content);
    }
    return;
  }
  if (type === "agent_settled") {
    clearAbortWatch(controller);
    controller.syncing = true;
    controller.working = false;
    controller.streaming = false;
    controller.stopping = false;
    controller.status = "";
    if (!isControllerSelected(controller)) controller.unread = true;
    watchSessionReplacement(controller);
    await rpc(controller, {
      id: nextRequestId("settled-state"),
      type: "get_state",
    });
    return;
  }
  // Pi announces every rename, whether it came from Tau's header, one of Pi's
  // own commands, or an extension, so the name is only ever read back from Pi.
  if (type === "session_info_changed") {
    applySessionName(controller, stringValue(event.name));
    await persistSessionName(controller);
    return;
  }
  if (type === "auto_retry_start") {
    controller.status = retryStatus(event);
    return;
  }
  if (type === "extension_error") {
    controller.status = stringValue(event.error) || "A Pi extension failed.";
  }
}

async function handleResponse(
  controller: SessionController,
  response: Record<string, unknown>,
) {
  const command = stringValue(response.command);
  const responseId = stringValue(response.id);
  const resolvesRunState =
    command === "get_state" &&
    Boolean(controller.runStateRequestId) &&
    responseId === controller.runStateRequestId;
  if (resolvesRunState) controller.runStateRequestId = "";
  const resolvesReplacementProbe =
    command === "get_state" &&
    Boolean(controller.replacementProbeRequestId) &&
    responseId === controller.replacementProbeRequestId;
  if (resolvesReplacementProbe) controller.replacementProbeRequestId = "";
  const resolvesCommandSync =
    command === "get_state" &&
    Boolean(controller.commandSyncRequestId) &&
    responseId === controller.commandSyncRequestId;
  if (resolvesCommandSync) controller.commandSyncRequestId = "";
  const resolvesAbortProbe =
    command === "get_state" &&
    Boolean(controller.abortProbeRequestId) &&
    responseId === controller.abortProbeRequestId;
  if (resolvesAbortProbe) controller.abortProbeRequestId = "";
  if (
    command === "prompt" &&
    Boolean(controller.commandPromptRequestId) &&
    responseId === controller.commandPromptRequestId
  ) {
    controller.commandPromptRequestId = "";
    if (response.success === true) {
      controller.working = controller.streaming;
      await syncAfterCommand(controller);
      return;
    }
  }
  if (response.success !== true) {
    const error = asRecord(response.error);
    controller.status =
      stringValue(error?.message) ||
      stringValue(response.error) ||
      `Pi rejected ${command || "the request"}.`;
    const pending = controller.pendingPrompt;
    const failedPendingSetting =
      Boolean(pending?.settingsRequestId) &&
      responseId === pending?.settingsRequestId;
    if (failedPendingSetting) {
      cancelPendingPrompt(controller, controller.status);
      return;
    }
    const failedPendingRequest =
      Boolean(pending) &&
      ((command === "get_state" && responseId === pending?.stateRequestId) ||
        (command === "get_messages" &&
          responseId === pending?.messagesRequestId));
    if (failedPendingRequest) {
      cancelPendingPrompt(controller, controller.status);
    }
    if (command === "get_state") {
      controller.syncing = false;
      if (responseId === controller.bootstrapStateRequestId) {
        controller.bootstrapStateRequestId = "";
        controller.starting = false;
      }
      releaseRuntime(controller);
    }
    if (
      command === "get_messages" &&
      responseId === controller.startMessagesRequestId
    ) {
      controller.startMessagesRequestId = "";
      controller.starting = false;
      controller.syncing = false;
    }
    if (command === "prompt") controller.working = false;
    if (command === "abort") controller.stopping = false;
    // A rejected rename leaves the optimistic name on screen, so Pi's name is
    // read back instead of being guessed.
    if (command === "set_session_name") {
      await rpc(controller, {
        id: nextRequestId("session-name-state"),
        type: "get_state",
      });
    }
    return;
  }
  const data = asRecord(response.data);

  if (command === "get_state" && data) {
    const model = asRecord(data.model);
    controller.currentModelProvider = stringValue(model?.provider);
    controller.currentModelId = stringValue(model?.id);
    controller.currentModelName = stringValue(model?.name);
    controller.currentEffort = normalizeEffort(data.thinkingLevel);
    const piSessionId = stringValue(data.sessionId);
    const piSessionPath = stringValue(data.sessionFile);
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.stateRequestId) &&
      responseId === pending?.stateRequestId;
    const resolvesBootstrap =
      Boolean(controller.bootstrapStateRequestId) &&
      responseId === controller.bootstrapStateRequestId;
    // Pi opens a session it cannot find as a fresh one under the very path it
    // was handed, so a bootstrap that answers with the requested path under
    // another id is Pi reporting that the session was never saved. Every other
    // session Pi reports here is one it holds, replacement included, since a
    // replacement always brings its own path.
    const unsavedSession =
      resolvesBootstrap &&
      !controller.phantom &&
      Boolean(controller.bootstrapSessionPath) &&
      piSessionPath === controller.bootstrapSessionPath &&
      Boolean(piSessionId) &&
      piSessionId !== controller.sessionId;
    const sessionChanged =
      !controller.phantom &&
      !unsavedSession &&
      Boolean(piSessionId && piSessionPath) &&
      (controller.sessionId !== piSessionId ||
        controller.sessionPath !== piSessionPath);
    controller.sessionName = stringValue(data.sessionName);
    controller.ready = true;
    controller.streaming = data.isStreaming === true;
    controller.stopping = false;
    controller.status =
      resolvesAbortProbe && controller.streaming
        ? unstoppedStatus(controller)
        : "";
    finishRemoteConnection(controller);

    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom && !unsavedSession) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }

    if (sessionChanged) {
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      controller.messages = [];
      controller.models = [];
      controller.efforts = [];
      controller.commands = [];
      controller.commandsLoaded = false;
      controller.pendingEffort = "";
      controller.syncing = true;
    }
    controller.working = controller.streaming || Boolean(pending);

    if (unsavedSession) await retireUnsavedSession(controller);
    if (controller.disposed) return;

    if (
      controller.sessionId &&
      controller.sessionPath &&
      (!resolvesRunState || sessionChanged) &&
      // A command's session is not final until Pi finishes handling it, so
      // registering now would record a session the command is about to
      // replace, and Pi never writes one that holds no assistant message.
      !(resolvesPending && pending?.command)
    ) {
      await registerConnectedSession(controller);
    }
    if (controller.disposed) return;

    if (resolvesPending && pending) {
      controller.bootstrapStateRequestId = "";
      await applyPendingSessionSettings(controller);
      return;
    }
    if (resolvesRunState && !sessionChanged) return;
    if (resolvesReplacementProbe && !sessionChanged) {
      releaseIdleRuntimes();
      return;
    }
    if (resolvesCommandSync && controller.streaming && !sessionChanged) return;
    // A run that outlived its abort is still writing the transcript, so the
    // probe only reports on it. A run that did stop falls through and syncs.
    if (resolvesAbortProbe && controller.streaming && !sessionChanged) return;

    if (sessionChanged) {
      await rpc(controller, {
        id: nextRequestId("replacement-models"),
        type: "get_available_models",
      });
      await rpc(controller, {
        id: nextRequestId("replacement-commands"),
        type: "get_commands",
      });
    }

    const messagesRequestId = nextRequestId("messages");
    if (resolvesBootstrap) {
      controller.startMessagesRequestId = messagesRequestId;
      controller.bootstrapStateRequestId = "";
    }
    await rpc(controller, {
      id: nextRequestId("efforts"),
      type: "get_available_thinking_levels",
    });
    await rpc(controller, { id: messagesRequestId, type: "get_messages" });
    return;
  }

  if (command === "get_messages" && data) {
    controller.syncing = false;
    controller.messages = appendLocalErrors(
      hydrateTranscript(
        Array.isArray(data.messages) ? data.messages : [],
        controller.messages,
      ),
      controller.localErrors,
    );
    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.messagesRequestId) &&
      responseId === pending?.messagesRequestId;
    if (resolvesPending) {
      await dispatchPendingPrompt(controller);
    } else if (responseId === controller.startMessagesRequestId) {
      controller.startMessagesRequestId = "";
      controller.starting = false;
    }
    // A hidden session that finishes hydrating has nothing left to wait for,
    // so this is where a runtime the user has moved on from is accounted for.
    releaseIdleRuntimes();
    return;
  }

  if (command === "get_available_models" && data) {
    controller.models = (Array.isArray(data.models) ? data.models : [])
      .map((value): ModelOption | undefined => {
        const model = asRecord(value);
        const provider = stringValue(model?.provider);
        const id = stringValue(model?.id);
        if (!provider || !id) return undefined;
        return {
          provider,
          id,
          name: stringValue(model?.name) || id,
          reasoning: model?.reasoning === true,
        };
      })
      .filter((model): model is ModelOption => Boolean(model));
    return;
  }

  if (command === "get_available_thinking_levels" && data) {
    controller.efforts = (Array.isArray(data.levels) ? data.levels : [])
      .map(normalizeEffort)
      .filter((level, index, levels) => levels.indexOf(level) === index);
    return;
  }

  if (command === "get_commands" && data) {
    controller.commands = (Array.isArray(data.commands) ? data.commands : [])
      .map(commandOption)
      .filter((command): command is CommandOption => Boolean(command));
    controller.commandsLoaded = true;
    return;
  }

  if (command === "prompt") {
    controller.working = controller.streaming;
    return;
  }

  if (command === "set_model") {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === "model" &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = "";
      pending.settingsStep = "";
      controller.currentModelProvider = pending.selectedModelProvider;
      controller.currentModelId = pending.selectedModelId;
      controller.currentModelName = pending.selectedModelName;
      await applyPendingSessionEffort(controller, true);
      return;
    }
    await rpc(controller, {
      id: nextRequestId("model-state"),
      type: "get_state",
    });
    return;
  }

  if (command === "set_thinking_level") {
    const pending = controller.pendingPrompt;
    if (
      pending?.settingsStep === "effort" &&
      responseId === pending.settingsRequestId
    ) {
      pending.settingsRequestId = "";
      pending.settingsStep = "";
      controller.currentEffort = pending.selectedEffort;
      controller.status = "";
      await requestPendingMessages(controller);
      return;
    }
    if (controller.pendingEffort) {
      controller.currentEffort = controller.pendingEffort;
    }
    controller.pendingEffort = "";
    controller.status = "";
  }
}

async function applyPendingSessionSettings(controller: SessionController) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  const modelChanged =
    Boolean(pending.selectedModelProvider && pending.selectedModelId) &&
    (pending.selectedModelProvider !== controller.currentModelProvider ||
      pending.selectedModelId !== controller.currentModelId);
  if (!modelChanged) {
    await applyPendingSessionEffort(controller, false);
    return;
  }

  const requestId = nextRequestId("initial-model");
  pending.settingsRequestId = requestId;
  pending.settingsStep = "model";
  await rpc(controller, {
    id: requestId,
    type: "set_model",
    provider: pending.selectedModelProvider,
    modelId: pending.selectedModelId,
  });
}

async function applyPendingSessionEffort(
  controller: SessionController,
  force: boolean,
) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  if (!force && pending.selectedEffort === controller.currentEffort) {
    await requestPendingMessages(controller);
    return;
  }

  const requestId = nextRequestId("initial-effort");
  pending.settingsRequestId = requestId;
  pending.settingsStep = "effort";
  await rpc(controller, {
    id: requestId,
    type: "set_thinking_level",
    level: pending.selectedEffort,
  });
}

async function requestPendingMessages(controller: SessionController) {
  const pending = controller.pendingPrompt;
  if (!pending) return;
  const messagesRequestId = nextRequestId("messages");
  pending.messagesRequestId = messagesRequestId;
  await rpc(controller, {
    id: nextRequestId("efforts"),
    type: "get_available_thinking_levels",
  });
  await rpc(controller, { id: messagesRequestId, type: "get_messages" });
}

async function dispatchPendingPrompt(controller: SessionController) {
  const prompt = controller.pendingPrompt;
  if (!prompt) return;
  controller.pendingPrompt = undefined;
  controller.starting = false;
  if (
    !prompt.command &&
    !controller.messages.some((message) => message.id === prompt.optimisticId)
  ) {
    controller.messages.push({
      id: prompt.optimisticId,
      kind: "user",
      text: prompt.message,
    });
  }
  controller.working = true;
  try {
    const requestId = nextRequestId("prompt");
    if (prompt.command) controller.commandPromptRequestId = requestId;
    await rpc(controller, {
      id: requestId,
      type: "prompt",
      message: prompt.message,
    });
  } catch (error) {
    controller.working = false;
    controller.commandPromptRequestId = "";
    controller.draft = prompt.message;
    setControllerError(controller, error);
  }
}

/**
 * Pi resolves an extension command's `prompt` only after the handler returns,
 * so any session it opened already exists by the time this runs. Nothing else
 * reports the swap, and the command itself never lands in the transcript.
 */
async function syncAfterCommand(controller: SessionController) {
  if (controller.disposed || !controller.generation) return;
  const requestId = nextRequestId("command-sync");
  controller.commandSyncRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch (error) {
    controller.commandSyncRequestId = "";
    setControllerError(controller, error);
  }
}

function watchSessionReplacement(controller: SessionController) {
  clearSessionReplacementWatch(controller);
  if (controller.disposed || !controller.generation) return;
  replacementProbeTimers.set(
    controller.key,
    replacementProbeDelays.map((delay, index) =>
      setTimeout(() => {
        void probeSessionReplacement(
          controller,
          index === replacementProbeDelays.length - 1,
        );
      }, delay),
    ),
  );
}

async function probeSessionReplacement(
  controller: SessionController,
  last: boolean,
) {
  if (last) replacementProbeTimers.delete(controller.key);
  if (
    controller.disposed ||
    !controller.generation ||
    controller.starting ||
    controller.streaming ||
    controller.working
  ) {
    return;
  }
  const requestId = nextRequestId("replacement-probe");
  controller.replacementProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch {
    controller.replacementProbeRequestId = "";
  }
}

function clearSessionReplacementWatch(controller: SessionController) {
  const timers = replacementProbeTimers.get(controller.key);
  if (!timers) return;
  replacementProbeTimers.delete(controller.key);
  for (const timer of timers) clearTimeout(timer);
}

function watchingSessionReplacement(controller: SessionController): boolean {
  return replacementProbeTimers.has(controller.key);
}

function watchAbort(controller: SessionController) {
  clearAbortWatch(controller);
  if (controller.disposed || !controller.generation) return;
  abortProbeTimers.set(
    controller.key,
    setTimeout(() => {
      void probeAbort(controller);
    }, abortAcknowledgeDelay),
  );
}

async function probeAbort(controller: SessionController) {
  abortProbeTimers.delete(controller.key);
  if (controller.disposed || !controller.generation || !controller.stopping) {
    return;
  }
  const requestId = nextRequestId("abort-probe");
  controller.abortProbeRequestId = requestId;
  try {
    await rpc(controller, { id: requestId, type: "get_state" });
  } catch (error) {
    controller.abortProbeRequestId = "";
    controller.stopping = false;
    setControllerError(controller, error);
  }
}

function clearAbortWatch(controller: SessionController) {
  const timer = abortProbeTimers.get(controller.key);
  if (!timer) return;
  abortProbeTimers.delete(controller.key);
  clearTimeout(timer);
}

/**
 * Naming the tool that is holding the run open turns a session that looks
 * frozen into one that is visibly waiting on something.
 */
function unstoppedStatus(controller: SessionController): string {
  const tool = [...controller.messages]
    .reverse()
    .find((entry) => entry.kind === "tool" && entry.toolRunning);
  return tool?.toolName
    ? `Pi is still running ${tool.toolName} and stops once it returns.`
    : "Pi has not stopped yet and stops once its current work finishes.";
}

/**
 * Extension commands are executed by Pi instead of being sent to the model, so
 * they never become session entries and must not be shown as user messages.
 */
function invokesExtensionCommand(
  controller: SessionController,
  message: string,
): boolean {
  if (!message.startsWith("/")) return false;
  // Pi takes the command name from the first space, so anything else is a
  // prompt for the model even when it opens with a slash.
  const space = message.indexOf(" ");
  const name = space < 0 ? message.slice(1) : message.slice(1, space);
  return (
    Boolean(name) &&
    controller.commands.some(
      (command) => command.name === name && command.source === "extension",
    )
  );
}

function applySessionName(controller: SessionController, name: string) {
  controller.sessionName = name;
  const session = ephemeralSessionByController(controller.key);
  if (name && session && !session.phantom) session.title = name;
}

async function persistSessionName(controller: SessionController) {
  if (controller.phantom || !controller.sessionId || !controller.sessionPath) {
    return;
  }
  await registerConnectedSession(controller);
}

async function registerConnectedSession(controller: SessionController) {
  try {
    const workspace = await invoke<WorkspaceSnapshot>("register_session", {
      projectPath: controller.projectPath,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
      sessionName:
        controller.sessionName || firstUserMessage(controller) || null,
      lastUserMessageAt:
        controller.lastUserMessageAt > 0 ? controller.lastUserMessageAt : null,
    });
    if (controller.disposed) return;
    state.workspace = workspace;
    if (workspaceContainsSession(controller)) {
      removeRegisteredEphemeralSession(controller);
    }
    if (isControllerSelected(controller)) {
      state.activeSessionId = controller.sessionId;
      state.activeSessionPath = controller.sessionPath;
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_session", {
        projectPath: controller.projectPath,
        sessionId: controller.sessionId,
      });
    }
  } catch (error) {
    setControllerError(controller, error);
  }
}

async function persistExpandedProject(projectPath: string) {
  try {
    state.workspace = await invoke<WorkspaceSnapshot>("set_project_collapsed", {
      path: projectPath,
      collapsed: false,
    });
  } catch (error) {
    setActiveError(error);
  }
}

async function persistProjectSelection(
  projectPath: string,
  controller: SessionController,
) {
  try {
    if (controller.phantom) {
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_project", {
        path: projectPath,
      });
    } else {
      state.workspace = await invoke<WorkspaceSnapshot>("set_active_session", {
        projectPath,
        sessionId: controller.sessionId,
      });
    }
  } catch (error) {
    setControllerError(controller, error);
  }
}

function materializePendingSession(
  controller: SessionController,
  sessionId: string,
  sessionPath: string,
) {
  const session = ephemeralSessionByController(controller.key);
  controller.sessionId = sessionId;
  controller.sessionPath = sessionPath;
  controller.phantom = false;
  if (session) {
    session.id = sessionId;
    session.path = sessionPath;
    session.phantom = false;
    session.title = draftTitle(controller.pendingPrompt?.message ?? "");
  }
  if (isControllerSelected(controller)) {
    state.activeSessionId = sessionId;
    state.activeSessionPath = sessionPath;
  }
}

function cancelPendingPrompt(controller: SessionController, error: unknown) {
  const prompt = controller.pendingPrompt;
  if (!prompt) {
    setControllerError(controller, error);
    return;
  }
  controller.pendingPrompt = undefined;
  controller.starting = false;
  controller.working = false;
  controller.messages = controller.messages.filter(
    (message) => message.id !== prompt.optimisticId,
  );
  controller.draft = prompt.message;
  setControllerError(controller, error);
}

/**
 * Shows a failure where the reply it replaced would have been. The row is one
 * Pi will send again with the settled transcript, so its id marks it as this
 * client's until the hydrated list takes it over.
 */
function pushError(controller: SessionController, text: string) {
  controller.messages.push({
    id: `stream-error-${controller.streamSequence++}`,
    kind: "error",
    text,
  });
  if (!isControllerSelected(controller)) controller.unread = true;
}

/** How much of a reason a retry line carries before the composer is pushed down. */
const retryReasonLimit = 120;

function retryStatus(event: Record<string, unknown>): string {
  const attempt = String(event.attempt ?? "");
  const maxAttempts = event.maxAttempts ? `/${String(event.maxAttempts)}` : "";
  const reason = describePiError(stringValue(event.errorMessage)).message;
  const trimmed =
    reason.length > retryReasonLimit
      ? `${reason.slice(0, retryReasonLimit).trimEnd()}…`
      : reason;
  return trimmed
    ? `Retrying (${attempt}${maxAttempts}): ${trimmed}`
    : `Retrying (${attempt}${maxAttempts})…`;
}

function appendStream(
  controller: SessionController,
  kind: "assistant" | "thinking",
  delta: string,
) {
  if (!delta) return;
  const last = controller.messages[controller.messages.length - 1];
  if (last?.kind === kind && last.id.startsWith("stream-")) {
    last.text += delta;
    return;
  }
  controller.messages.push({
    id: `stream-${kind}-${controller.streamSequence++}`,
    kind,
    text: delta,
  });
}

function canArchiveSession(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return ephemeralSession(project.path, session.id)?.phantom !== true;
}

function projectSessions(project: ProjectSummary): SessionSummary[] {
  const ephemeral = state.ephemeralSessions.filter(
    (session) => session.projectPath === project.path,
  );
  const ephemeralIds = new Set(ephemeral.map((session) => session.id));
  const phantomCreatedAt = new Map(
    ephemeral
      .filter((session) => session.phantom)
      .map((session) => [session.id, session.createdAt]),
  );
  return [
    ...ephemeral,
    ...project.sessions.filter(
      (session) => !session.archived && !ephemeralIds.has(session.id),
    ),
  ].sort((left, right) => {
    const leftPhantomCreatedAt = phantomCreatedAt.get(left.id);
    const rightPhantomCreatedAt = phantomCreatedAt.get(right.id);
    if (
      leftPhantomCreatedAt !== undefined ||
      rightPhantomCreatedAt !== undefined
    ) {
      if (leftPhantomCreatedAt === undefined) return 1;
      if (rightPhantomCreatedAt === undefined) return -1;
      return rightPhantomCreatedAt - leftPhantomCreatedAt;
    }
    return (
      sessionLastUserMessageAt(project.path, right) -
      sessionLastUserMessageAt(project.path, left)
    );
  });
}

function sessionLastActive(
  project: ProjectSummary,
  session: SessionSummary,
): string {
  const timestamp = sessionLastUserMessageAt(project.path, session);
  return timestamp > session.lastUserMessageAt
    ? relativeTimestamp(timestamp)
    : session.lastActive;
}

function sessionLastUserMessageAt(
  projectPath: string,
  session: SessionSummary,
): number {
  return Math.max(
    session.lastUserMessageAt,
    controllerForSession(projectPath, session.id)?.lastUserMessageAt ?? 0,
  );
}

function isSessionSelected(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return (
    project.path === state.activeProjectPath &&
    session.id === state.activeSessionId
  );
}

/**
 * A waiting prompt reads as unread rather than working: it arrives mid-turn,
 * so the working state would otherwise bury the one thing that needs an
 * answer before the session can move.
 *
 * Background activity only ever marks an unselected session unread, so an
 * unread flag on the selected session is a deliberate mark and stays visible
 * until the session is read again.
 */
function sessionIndicator(
  project: ProjectSummary,
  session: SessionSummary,
): SessionIndicator {
  const controller = controllerForSession(project.path, session.id);
  if (!controller) return "";
  const selected = isSessionSelected(project, session);
  if (!selected && controllerHasPendingDialog(controller)) return "new";
  if (controller.working) return "working";
  if (controller.draft.trim()) return "draft";
  return controller.unread ? "new" : "";
}

function isSessionUnread(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return controllerForSession(project.path, session.id)?.unread === true;
}

/**
 * Unread is otherwise a by-product of background activity, so marking a
 * session Tau has never opened needs a controller to hold the flag. The
 * controller stays idle until the session is selected and started.
 */
function markSessionUnread(project: ProjectSummary, session: SessionSummary) {
  ensureController(project, session).unread = true;
}

function markSessionRead(project: ProjectSummary, session: SessionSummary) {
  const controller = controllerForSession(project.path, session.id);
  if (controller) controller.unread = false;
}

function indicatorLabel(indicator: SessionIndicator): string {
  return indicator ? indicatorLabels[indicator] : "";
}

/**
 * Collapsed projects hide their sessions, so the rolled-up indicator is the
 * only place an unread session or a waiting prompt can surface.
 */
function projectIndicator(project: ProjectSummary): SessionIndicator {
  if (!project.collapsed) return "";
  const indicators = new Set(
    projectSessions(project).map((session) =>
      sessionIndicator(project, session),
    ),
  );
  for (const indicator of ["new", "draft", "working"] as const) {
    if (indicators.has(indicator)) return indicator;
  }
  return "";
}

function setActiveSessionView(
  project: ProjectSummary,
  session: SessionSummary,
  controller: SessionController,
) {
  state.activeProjectPath = project.path;
  state.activeSessionId = session.id;
  state.activeSessionPath = session.path;
  state.activeControllerKey = controller.key;
  controller.unread = false;
  touchController(controller);
}

function createPhantomSession(
  projectPath: string,
  controllerKey: string,
): EphemeralSession {
  phantomSequence += 1;
  const now = Date.now();
  return {
    id: `phantom-${now}-${phantomSequence}`,
    path: "",
    title: "New session",
    lastActive: "now",
    lastUserMessageAt: 0,
    archived: false,
    selected: true,
    projectPath,
    controllerKey,
    createdAt: now * 1000 + phantomSequence,
    phantom: true,
  };
}

/**
 * Pi writes a session file only once the session holds an assistant message,
 * so a session Tau registered from a workflow handoff that was cancelled
 * before it answered exists in the sidebar and nowhere else. Pi opens that
 * path as a brand new session, and registering the id it comes back with
 * files a second session at the same path, so the row spawns one more on
 * every visit and none of them can ever be opened.
 *
 * The row is retired instead, and the empty session Pi did open is shown as
 * what it is: an unsent session that leaves no trace unless it is used.
 */
async function retireUnsavedSession(controller: SessionController) {
  const staleSessionId = controller.sessionId;
  if (workspaceContainsSession(controller)) {
    try {
      state.workspace = await invoke<WorkspaceSnapshot>("archive_session", {
        projectPath: controller.projectPath,
        sessionId: staleSessionId,
      });
    } catch {
      // A row Tau cannot retire is still worth replacing with a session that
      // works, and the message below says why it is there.
    }
  }
  if (controller.disposed) return;

  const previous = ephemeralSessionByController(controller.key);
  if (previous) removeEphemeralSession(previous, false);
  const session = createPhantomSession(controller.projectPath, controller.key);
  state.ephemeralSessions.push(session);

  controller.phantom = true;
  controller.sessionId = session.id;
  controller.sessionPath = "";
  controller.sessionName = "";
  controller.lastUserMessageAt = 0;
  controller.messages = [];
  if (isControllerSelected(controller)) {
    state.activeSessionId = session.id;
    state.activeSessionPath = "";
  }
  controller.status =
    "That session was never saved by Pi, so this is a new one.";
}

function removeEmptyActivePhantom() {
  const controller = activeController.value;
  const session = controller
    ? ephemeralSessionByController(controller.key)
    : undefined;
  if (
    !controller ||
    !session?.phantom ||
    controller.draft.trim() ||
    controller.working ||
    controllerHasPendingDialog(controller) ||
    controller.messages.length > 0
  ) {
    return;
  }
  removeEphemeralSession(session, true);
}

function removeRegisteredEphemeralSession(controller: SessionController) {
  const session = ephemeralSessionByController(controller.key);
  if (session && !session.phantom) removeEphemeralSession(session, false);
}

function workspaceContainsSession(controller: SessionController): boolean {
  return (
    state.workspace?.projects
      .find((project) => project.path === controller.projectPath)
      ?.sessions.some((session) => session.id === controller.sessionId) === true
  );
}

function removeEphemeralSession(
  session: EphemeralSession,
  removeController: boolean,
) {
  const index = state.ephemeralSessions.indexOf(session);
  if (index >= 0) state.ephemeralSessions.splice(index, 1);
  if (removeController) {
    const controller = controllerByKey(session.controllerKey);
    if (controller) {
      controller.disposed = true;
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      void stopControllerProcess(controller);
      const controllerIndex = state.controllers.indexOf(controller);
      if (controllerIndex >= 0) state.controllers.splice(controllerIndex, 1);
      if (state.activeControllerKey === controller.key) clearActiveSession();
    }
  }
}

function removeProjectUiState(projectPath: string) {
  state.ephemeralSessions = state.ephemeralSessions.filter(
    (session) => session.projectPath !== projectPath,
  );
  for (const controller of state.controllers) {
    if (controller.projectPath !== projectPath) continue;
    controller.disposed = true;
    clearSessionReplacementWatch(controller);
    clearAbortWatch(controller);
  }
  state.controllers = state.controllers.filter(
    (controller) => controller.projectPath !== projectPath,
  );
}

function ensureController(
  project: ProjectSummary,
  session: SessionSummary,
): SessionController {
  return (
    controllerForSession(project.path, session.id) ??
    createController(project, session, nextControllerKey())
  );
}

function inheritControllerSettings(
  controller: SessionController,
  preferred: SessionController | undefined,
) {
  const source =
    preferred &&
    (preferred.models.length > 0 ||
      preferred.efforts.length > 0 ||
      preferred.currentModelId)
      ? preferred
      : [...state.controllers]
          .reverse()
          .find(
            (candidate) =>
              candidate.key !== controller.key &&
              (candidate.models.length > 0 ||
                candidate.efforts.length > 0 ||
                candidate.currentModelId),
          );
  if (source) {
    controller.models = [...source.models];
    // The scope belongs to the catalogue it narrows. A session that inherits
    // one without the other never starts a runtime to read the scope back, and
    // would offer every model Pi knows.
    controller.modelScope = [...source.modelScope];
    controller.efforts = [...source.efforts];
    controller.currentModelProvider = source.currentModelProvider;
    controller.currentModelId = source.currentModelId;
    controller.currentModelName = source.currentModelName;
    controller.currentEffort = source.currentEffort;
  }

  const commandSource = [...state.controllers]
    .reverse()
    .find(
      (candidate) =>
        candidate.key !== controller.key &&
        candidate.projectPath === controller.projectPath &&
        candidate.commandsLoaded,
    );
  if (commandSource) {
    controller.commands = [...commandSource.commands];
    controller.commandsLoaded = true;
  }
}

function createController(
  project: ProjectSummary,
  session: SessionSummary,
  key: string,
): SessionController {
  const phantom =
    ("phantom" in session && session.phantom === true) ||
    ephemeralSession(project.path, session.id)?.phantom === true;
  const controller: SessionController = reactive({
    key,
    runtimeId: `runtime-${key}`,
    projectPath: project.path,
    sessionId: session.id,
    sessionPath: session.path,
    sessionName: session.title === "New session" ? "" : session.title,
    phantom,
    generation: 0,
    ready: false,
    streaming: false,
    stopping: false,
    starting: false,
    working: false,
    unread: false,
    lastUserMessageAt: session.lastUserMessageAt,
    messages: [],
    localErrors: [],
    draft: "",
    status: "",
    currentModelProvider: "",
    currentModelId: "",
    currentModelName: "",
    currentEffort: "off",
    pendingEffort: "",
    models: [],
    modelScope: [],
    efforts: [],
    commands: [],
    commandsLoaded: false,
    pendingPrompt: undefined,
    bootstrapStateRequestId: "",
    bootstrapSessionPath: "",
    runStateRequestId: "",
    startMessagesRequestId: "",
    commandPromptRequestId: "",
    commandSyncRequestId: "",
    replacementProbeRequestId: "",
    abortProbeRequestId: "",
    connectingRemote: false,
    syncing: false,
    lastActiveSequence: (activitySequence += 1),
    disposed: false,
    streamSequence: 0,
  });
  state.controllers.push(controller);
  return controller;
}

function controllerForSession(
  projectPath: string,
  sessionId: string,
): SessionController | undefined {
  const ephemeral = ephemeralSession(projectPath, sessionId);
  if (ephemeral) return controllerByKey(ephemeral.controllerKey);
  return state.controllers.find(
    (controller) =>
      controller.projectPath === projectPath &&
      controller.sessionId === sessionId,
  );
}

function controllerByKey(key: string): SessionController | undefined {
  return state.controllers.find((controller) => controller.key === key);
}

function controllerByRuntimeId(
  runtimeId: string,
): SessionController | undefined {
  return state.controllers.find(
    (controller) => controller.runtimeId === runtimeId,
  );
}

function ephemeralSession(
  projectPath: string,
  sessionId: string,
): EphemeralSession | undefined {
  return state.ephemeralSessions.find(
    (session) =>
      session.projectPath === projectPath && session.id === sessionId,
  );
}

function ephemeralSessionByController(
  controllerKey: string,
): EphemeralSession | undefined {
  return state.ephemeralSessions.find(
    (session) => session.controllerKey === controllerKey,
  );
}

function isControllerSelected(controller: SessionController): boolean {
  return controller.key === state.activeControllerKey;
}

function controllerHasPendingDialog(controller: SessionController): boolean {
  return state.extensionDialogs.some(
    (dialog) => dialog.controllerKey === controller.key,
  );
}

function runtimeAvailable(project: ProjectSummary): boolean {
  if (project.connectionString) return true;
  return Boolean(state.workspace?.piPath);
}

function canReleaseRuntime(controller: SessionController): boolean {
  return (
    !isControllerSelected(controller) &&
    controller.ready &&
    !controller.streaming &&
    !controller.working &&
    !controller.starting &&
    !controller.syncing &&
    !controllerHasPendingDialog(controller) &&
    !watchingSessionReplacement(controller)
  );
}

/**
 * Release a runtime whose session no longer warrants one, such as an archived
 * session or one whose runtime just failed a request.
 */
function releaseRuntime(controller: SessionController | undefined) {
  if (!controller || !canReleaseRuntime(controller)) return;
  void stopControllerProcess(controller);
}

/** Keep the most recently active idle runtimes warm and release the rest. */
function releaseIdleRuntimes() {
  const idle = state.controllers
    .filter((controller) => canReleaseRuntime(controller))
    .sort(
      (first, second) => second.lastActiveSequence - first.lastActiveSequence,
    );
  for (const controller of idle.slice(idleRuntimeLimit)) {
    void stopControllerProcess(controller);
  }
}

/** Record a runtime as the most recently active one. */
function touchController(controller: SessionController) {
  activitySequence += 1;
  controller.lastActiveSequence = activitySequence;
}

async function stopControllerProcess(controller: SessionController) {
  if (!controller.generation && !controller.starting) return;
  const generation = controller.generation;
  clearSessionReplacementWatch(controller);
  clearAbortWatch(controller);
  discardControllerDialogs(controller, generation);
  try {
    await invoke("stop_pi", { runtimeId: controller.runtimeId });
  } catch (error) {
    setControllerError(controller, error);
  }
  if (controller.generation !== generation) return;
  controller.generation = 0;
  controller.ready = false;
  controller.streaming = false;
  controller.stopping = false;
  controller.starting = false;
  controller.working = false;
  controller.connectingRemote = false;
  controller.syncing = false;
  controller.runStateRequestId = "";
  controller.abortProbeRequestId = "";

  if (
    isControllerSelected(controller) &&
    !controller.disposed &&
    !controller.phantom
  ) {
    const project = state.workspace?.projects.find(
      (item) => item.path === controller.projectPath,
    );
    if (project) {
      void startController(controller, project, controller.sessionPath);
    }
  }
}

function markUserMessageSubmitted(controller: SessionController) {
  controller.lastUserMessageAt = Date.now();
  const session = ephemeralSessionByController(controller.key);
  if (session) {
    session.lastUserMessageAt = controller.lastUserMessageAt;
    session.lastActive = "now";
  }
}

function relativeTimestamp(timestamp: number): string {
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1_000));
  if (seconds < 60) return "now";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`;
  if (seconds < 604_800) return `${Math.floor(seconds / 86_400)}d`;
  if (seconds < 31_536_000) return `${Math.floor(seconds / 604_800)}w`;
  return `${Math.floor(seconds / 31_536_000)}y`;
}

function draftTitle(value: string): string {
  return normalizeSessionName(value) || "New session";
}

function normalizeSessionName(value: string): string {
  return value.replace(/\s+/g, " ").trim().slice(0, 240);
}

function commandOption(value: unknown): CommandOption | undefined {
  const command = asRecord(value);
  const name = stringValue(command?.name);
  const source = stringValue(command?.source);
  if (
    !name ||
    (source !== "extension" && source !== "prompt" && source !== "skill")
  ) {
    return undefined;
  }

  const description = stringValue(command?.description);
  return {
    name,
    ...(description ? { description } : {}),
    source,
  };
}

function normalizeEffort(value: unknown): ThinkingLevel {
  return value === "minimal" ||
    value === "low" ||
    value === "medium" ||
    value === "high" ||
    value === "xhigh" ||
    value === "max"
    ? value
    : "off";
}

function nextRequestId(label: string): string {
  state.requestSequence += 1;
  return `tau-${label}-${state.requestSequence}`;
}

function nextControllerKey(): string {
  controllerSequence += 1;
  return `${Date.now()}-${controllerSequence}`;
}

function firstUserMessage(controller: SessionController | undefined): string {
  return (
    controller?.messages.find((message) => message.kind === "user")?.text ?? ""
  );
}

function clearActiveSession() {
  state.activeProjectPath = "";
  state.activeSessionId = "";
  state.activeSessionPath = "";
  state.activeControllerKey = "";
}

function clearRemoteDirectoryBrowser() {
  state.remoteDirectoryHost = "";
  state.remoteDirectoryRoot = "";
  state.remoteWorkingDirectory = "";
  state.remoteDirectoryHistory = [];
  state.remoteDirectories = [];
  state.remoteDirectoryFilter = "";
  state.remoteDirectorySelectedIndex = 0;
}

function applyRemoteDirectoryListing(
  listing: RemoteDirectoryListing,
  initial = false,
) {
  state.remoteConnectionString = listing.connectionString;
  state.remoteDirectoryHost = listing.host;
  if (initial) {
    state.remoteDirectoryRoot = listing.workingDirectory;
    state.remoteDirectoryHistory = [];
  }
  state.remoteWorkingDirectory = listing.workingDirectory;
  state.remoteDirectories = listing.directories;
  state.remoteDirectoryFilter = "";
  state.remoteDirectorySelectedIndex = 0;
  state.remoteConnectionError = "";
}

function presentRemoteConnectionError(
  controller: SessionController,
  error: unknown,
) {
  controller.ready = false;
  controller.streaming = false;
  controller.stopping = false;
  controller.starting = false;
  controller.working = false;
  controller.connectingRemote = false;
  controller.syncing = false;
  controller.runStateRequestId = "";
  controller.status = errorMessage(error);
  if (!isControllerSelected(controller)) return;

  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project?.connectionString) return;
  remoteRetry = {
    controllerKey: controller.key,
    projectPath: project.path,
    sessionPath: controller.sessionPath || undefined,
    preserveMessages: controller.messages.length > 0,
  };
  state.remoteDialogMode = "retry";
  state.remoteDialogStep = "connection";
  state.remoteConnectionString = project.connectionString;
  clearRemoteDirectoryBrowser();
  state.remoteConnectionError = controller.status;
  state.remoteConnecting = false;
  state.remoteDialogOpen = true;
}

function finishRemoteConnection(controller: SessionController) {
  controller.connectingRemote = false;
  if (remoteRetry?.controllerKey !== controller.key) return;
  remoteRetry = undefined;
  state.remoteConnecting = false;
  if (state.remoteDialogMode === "retry") {
    state.remoteDialogOpen = false;
    state.remoteConnectionError = "";
    clearRemoteDirectoryBrowser();
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function setControllerError(controller: SessionController, error: unknown) {
  controller.status = errorMessage(error);
}

function setActiveError(error: unknown) {
  const controller = activeController.value;
  if (controller) setControllerError(controller, error);
  else state.workspaceStatus = errorMessage(error);
}
