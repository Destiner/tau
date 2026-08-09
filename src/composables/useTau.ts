import { computed, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  asRecord,
  hydrateTranscript,
  stringValue,
  toolArgument,
} from "../lib/transcript";
import { getActivePiIntegration } from "../lib/pi-integrations";
import type {
  ModelOption,
  PiBridgeEvent,
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

type SessionIndicator = "new" | "draft" | "working" | "";

interface EphemeralSession extends SessionSummary {
  projectPath: string;
  controllerKey: string;
  createdAt: number;
  phantom: boolean;
}

interface PendingPrompt {
  message: string;
  optimisticId: string;
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
  draft: string;
  status: string;
  currentModelProvider: string;
  currentModelId: string;
  currentModelName: string;
  currentEffort: ThinkingLevel;
  pendingEffort: ThinkingLevel | "";
  models: ModelOption[];
  efforts: ThinkingLevel[];
  pendingPrompt?: PendingPrompt;
  bootstrapStateRequestId: string;
  startMessagesRequestId: string;
  connectingRemote: boolean;
  syncing: boolean;
  evictAfterHydration: boolean;
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

let unlisten: UnlistenFn | undefined;
let phantomSequence = 0;
let controllerSequence = 0;
let remoteRetry: RemoteRetry | undefined;

const emptyMessages: TranscriptEntry[] = [];
const emptyModels: ModelOption[] = [];
const emptyEfforts: ThinkingLevel[] = [];

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
const models = computed(() => activeController.value?.models ?? emptyModels);
const efforts = computed(() => activeController.value?.efforts ?? emptyEfforts);
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

export function useTau() {
  async function initialize() {
    if (!unlisten) {
      unlisten = await listen<PiBridgeEvent>("pi-event", ({ payload }) => {
        void handleBridgeEvent(payload).catch((error) => {
          const controller = controllerByRuntimeId(payload.runtimeId);
          if (!controller) return;
          controller.syncing = false;
          setControllerError(controller, error);
          maybeEvictController(controller);
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
        if (
          needsLocalRuntime &&
          getActivePiIntegration().requiresSdk &&
          !state.workspace.sdkAvailable
        ) {
          controller.status =
            "The SDK sidecar needs an npm-installed Pi package and Node.js.";
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

  async function newSession(project: ProjectSummary) {
    const previous = activeController.value;
    removeEmptyActivePhantom();
    phantomSequence += 1;
    const now = Date.now();
    const controllerKey = nextControllerKey();
    const id = `phantom-${now}-${phantomSequence}`;
    const session: EphemeralSession = {
      id,
      path: "",
      title: "New session",
      lastActive: "now",
      lastUserMessageAt: 0,
      archived: false,
      selected: true,
      projectPath: project.path,
      controllerKey,
      createdAt: now * 1000 + phantomSequence,
      phantom: true,
    };
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
    maybeEvictController(previous);
    if (
      runtimeAvailable(project) &&
      (!controller.currentModelId ||
        controller.models.length === 0 ||
        controller.efforts.length === 0)
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
        controller.evictAfterHydration = false;
        if (!controller.phantom && !controller.ready && !controller.starting) {
          await startController(controller, project, session.path);
        }
      }
      return;
    }

    const previous = activeController.value;
    removeEmptyActivePhantom();
    const controller = ensureController(project, session);
    setActiveSessionView(project, session, controller);
    controller.status = "";
    controller.unread = false;
    controller.evictAfterHydration = false;
    await persistProjectSelection(project.path, controller);
    maybeEvictController(previous);

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

    markUserMessageSubmitted(controller);

    if (controller.phantom) {
      await sendPhantomMessage(controller, message);
      return;
    }

    controller.draft = "";
    controller.working = true;
    controller.messages.push({
      id: `optimistic-user-${Date.now()}`,
      kind: "user",
      text: message,
    });
    controller.status = "";
    try {
      await rpc(controller, {
        id: nextRequestId("prompt"),
        type: "prompt",
        message,
      });
      await registerConnectedSession(controller);
    } catch (error) {
      controller.working = false;
      setControllerError(controller, error);
    }
  }

  async function stop() {
    const controller = activeController.value;
    if (!controller?.streaming || controller.stopping) return;
    controller.stopping = true;
    controller.status = "";
    try {
      await rpc(controller, { id: nextRequestId("abort"), type: "abort" });
    } catch (error) {
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
    initialize,
    dispose,
    addLocalProject,
    openRemoteProjectDialog,
    closeRemoteProjectDialog,
    submitRemoteConnection,
    chooseRemoteDirectory,
    toggleProject,
    removeProject,
    newSession,
    selectSession,
    projectSessions,
    sessionLastActive,
    isSessionSelected,
    sessionIndicator,
    sendMessage,
    stop,
    selectModel,
    selectEffort,
  };
}

async function sendPhantomMessage(
  controller: SessionController,
  message: string,
) {
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project || !controller.phantom) return;

  const optimisticId = `optimistic-user-${Date.now()}`;
  controller.pendingPrompt = {
    message,
    optimisticId,
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
  controller.messages.push({ id: optimisticId, kind: "user", text: message });
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
  controller.startMessagesRequestId = "";
  controller.evictAfterHydration = false;
  if (!preserveMessages && controller.messages.length === 0) {
    controller.messages = [];
  }
  controller.models = [];
  controller.efforts = [];
  controller.status = "";

  try {
    if (project.connectionString) {
      controller.generation = await invoke<number>("start_pi_remote", {
        runtimeId: controller.runtimeId,
        connectionString: project.connectionString,
        workingDirectory: project.workingDirectory,
        sessionPath: sessionPath ?? null,
      });
    } else {
      controller.generation = await invoke<number>(
        getActivePiIntegration().startCommand,
        {
          runtimeId: controller.runtimeId,
          projectPath: project.workingDirectory,
          sessionPath: sessionPath ?? null,
        },
      );
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

async function requestBootstrap(controller: SessionController) {
  await rpc(controller, {
    id: nextRequestId("models"),
    type: "get_available_models",
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

async function handleBridgeEvent(event: PiBridgeEvent) {
  const controller = controllerByRuntimeId(event.runtimeId);
  if (!controller) return;
  if (event.kind === "started") {
    if (event.generation >= controller.generation) {
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
    controller.generation = 0;
    controller.status = message;
  }
}

async function handleRpc(controller: SessionController, value: unknown) {
  const event = asRecord(value);
  if (!event) return;
  const type = stringValue(event.type);
  if (type === "response") {
    await handleResponse(controller, event);
    return;
  }
  if (type === "agent_start") {
    controller.streaming = true;
    controller.stopping = false;
    controller.working = true;
    controller.status = "";
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
  if (type === "tool_execution_start") {
    const toolCallId = stringValue(event.toolCallId);
    const toolName = stringValue(event.toolName) || "tool";
    controller.messages.push({
      id: `stream-tool-${controller.streamSequence++}`,
      kind: "tool",
      text: toolArgument(event.args),
      toolCallId,
      toolName,
      toolRunning: true,
      toolErrored: false,
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
    }
    return;
  }
  if (type === "agent_settled") {
    controller.syncing = true;
    controller.working = false;
    controller.streaming = false;
    controller.stopping = false;
    controller.status = "";
    if (!isControllerSelected(controller)) {
      controller.unread = true;
      controller.evictAfterHydration = true;
    }
    await rpc(controller, {
      id: nextRequestId("settled-state"),
      type: "get_state",
    });
    return;
  }
  if (type === "auto_retry_start") {
    controller.status = `Retrying (${String(event.attempt ?? "")})…`;
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
      maybeEvictController(controller);
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
    controller.sessionName = stringValue(data.sessionName);
    controller.ready = true;
    controller.streaming = data.isStreaming === true;
    controller.stopping = false;
    controller.status = "";
    finishRemoteConnection(controller);

    const pending = controller.pendingPrompt;
    const resolvesPending =
      Boolean(pending?.stateRequestId) &&
      responseId === pending?.stateRequestId;
    const resolvesBootstrap =
      Boolean(controller.bootstrapStateRequestId) &&
      responseId === controller.bootstrapStateRequestId;

    if (resolvesPending && pending) {
      materializePendingSession(controller, piSessionId, piSessionPath);
    } else if (piSessionId && !controller.phantom) {
      controller.sessionId = piSessionId;
      controller.sessionPath = piSessionPath;
    }

    controller.working = controller.streaming || Boolean(pending);

    if (controller.sessionId && controller.sessionPath) {
      await registerConnectedSession(controller);
    }
    if (controller.disposed) return;

    if (resolvesPending && pending) {
      controller.bootstrapStateRequestId = "";
      await applyPendingSessionSettings(controller);
      return;
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
    controller.messages = hydrateTranscript(
      Array.isArray(data.messages) ? data.messages : [],
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
      if (!isControllerSelected(controller)) {
        controller.evictAfterHydration = true;
      }
    }
    if (
      !isControllerSelected(controller) &&
      !controller.starting &&
      !controller.streaming &&
      !controller.working
    ) {
      controller.evictAfterHydration = true;
    }
    if (
      controller.evictAfterHydration &&
      !isControllerSelected(controller) &&
      !controller.streaming &&
      !controller.working
    ) {
      controller.evictAfterHydration = false;
      await stopControllerProcess(controller);
    }
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
    await rpc(controller, {
      id: nextRequestId("prompt"),
      type: "prompt",
      message: prompt.message,
    });
  } catch (error) {
    controller.working = false;
    controller.draft = prompt.message;
    setControllerError(controller, error);
  }
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

function sessionIndicator(
  project: ProjectSummary,
  session: SessionSummary,
): SessionIndicator {
  const controller = controllerForSession(project.path, session.id);
  if (controller?.working) return "working";
  if (controller?.draft.trim()) return "draft";
  return controller?.unread && !isSessionSelected(project, session)
    ? "new"
    : "";
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
    if (controller.projectPath === projectPath) controller.disposed = true;
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
  if (!source) return;
  controller.models = [...source.models];
  controller.efforts = [...source.efforts];
  controller.currentModelProvider = source.currentModelProvider;
  controller.currentModelId = source.currentModelId;
  controller.currentModelName = source.currentModelName;
  controller.currentEffort = source.currentEffort;
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
    draft: "",
    status: "",
    currentModelProvider: "",
    currentModelId: "",
    currentModelName: "",
    currentEffort: "off",
    pendingEffort: "",
    models: [],
    efforts: [],
    pendingPrompt: undefined,
    bootstrapStateRequestId: "",
    startMessagesRequestId: "",
    connectingRemote: false,
    syncing: false,
    evictAfterHydration: false,
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

function runtimeAvailable(project: ProjectSummary): boolean {
  if (project.connectionString) return true;
  return (
    Boolean(state.workspace?.piPath) &&
    (!getActivePiIntegration().requiresSdk ||
      Boolean(state.workspace?.sdkAvailable))
  );
}

function maybeEvictController(controller: SessionController | undefined) {
  if (
    !controller ||
    isControllerSelected(controller) ||
    !controller.ready ||
    controller.streaming ||
    controller.working ||
    controller.starting ||
    controller.syncing
  ) {
    return;
  }
  void stopControllerProcess(controller);
}

async function stopControllerProcess(controller: SessionController) {
  if (!controller.generation && !controller.starting) return;
  const generation = controller.generation;
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
  return value.replace(/\s+/g, " ").trim().slice(0, 240) || "New session";
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
