import { computed, reactive, watch } from "vue";
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
  createdAt: number;
  phantom: boolean;
}

interface SessionActivity {
  unread: boolean;
  working: boolean;
}

interface PendingPrompt {
  projectPath: string;
  phantomId: string;
  message: string;
  optimisticId: string;
  stateRequestId: string;
  messagesRequestId: string;
  actualSessionId: string;
}

const state = reactive({
  workspace: null as WorkspaceSnapshot | null,
  messages: [] as TranscriptEntry[],
  draft: "",
  status: "",
  piReady: false,
  streaming: false,
  stopping: false,
  startingSession: false,
  switchingSession: false,
  activeGeneration: 0,
  activeProjectPath: "",
  activeSessionId: "",
  activeSessionPath: "",
  sessionName: "",
  piProjectPath: "",
  piSessionId: "",
  piSessionPath: "",
  piSessionName: "",
  currentModelProvider: "",
  currentModelId: "",
  currentModelName: "",
  currentEffort: "off" as ThinkingLevel,
  pendingEffort: "" as ThinkingLevel | "",
  models: [] as ModelOption[],
  efforts: [] as ThinkingLevel[],
  ephemeralSessions: [] as EphemeralSession[],
  requestSequence: 0,
  streamSequence: 0,
});

const sessionDrafts = reactive<Record<string, string>>({});
const sessionActivity = reactive<Record<string, SessionActivity>>({});

let unlisten: UnlistenFn | undefined;
let phantomSequence = 0;
let pendingPrompt: PendingPrompt | undefined;
let selectionStateRequestId = "";

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

const activeIsPhantom = computed(() =>
  isPhantomSession(state.activeProjectPath, state.activeSessionId),
);

const runtimeAvailable = computed(
  () =>
    Boolean(state.workspace?.piPath) &&
    (!getActivePiIntegration().requiresSdk ||
      Boolean(state.workspace?.sdkAvailable)),
);

const canCompose = computed(() => {
  if (!activeProject.value || !activeSession.value) return false;
  if (state.startingSession || state.switchingSession) return false;
  return activeIsPhantom.value ? runtimeAvailable.value : state.piReady;
});

const sessionTitle = computed(
  () =>
    state.sessionName ||
    activeSession.value?.title ||
    firstUserMessage() ||
    "New session",
);

const currentModelLabel = computed(
  () => state.currentModelId || state.currentModelName || "Model",
);

const currentEffortLabel = computed(() => effortLabels[state.currentEffort]);

const settingsDisabled = computed(
  () =>
    !state.piReady ||
    activeIsPhantom.value ||
    state.streaming ||
    state.stopping ||
    state.startingSession ||
    state.switchingSession,
);

watch(
  () => state.draft,
  (draft) => {
    const key = activeSessionKey();
    if (!key) return;
    sessionDrafts[key] = draft;
    const session = ephemeralSession(
      state.activeProjectPath,
      state.activeSessionId,
    );
    if (session?.phantom) session.title = draftTitle(draft);
  },
  { flush: "sync" },
);

export function useTau() {
  async function initialize() {
    if (!unlisten) {
      unlisten = await listen<PiBridgeEvent>("pi-event", ({ payload }) => {
        void handleBridgeEvent(payload);
      });
    }
    try {
      state.workspace = await invoke<WorkspaceSnapshot>("load_workspace");
      state.activeProjectPath = state.workspace.activeProjectPath;
      if (!state.workspace.piPath) {
        state.status =
          "Pi was not found. Install pi or set TAU_PI_PATH, then restart Tau.";
        return;
      }
      if (
        getActivePiIntegration().requiresSdk &&
        !state.workspace.sdkAvailable
      ) {
        state.status =
          "The SDK sidecar needs an npm-installed Pi package and Node.js.";
        return;
      }
      const selectedProject = state.workspace.projects.find(
        (project) => project.selected,
      );
      const selectedSession = selectedProject?.sessions.find(
        (session) => session.selected,
      );
      if (selectedProject && selectedSession) {
        setActiveSessionView(selectedProject, selectedSession);
        state.switchingSession = true;
        await startProject(selectedProject, selectedSession.path);
      }
    } catch (error) {
      state.startingSession = false;
      state.switchingSession = false;
      setError(error);
    }
  }

  function dispose() {
    unlisten?.();
    unlisten = undefined;
  }

  async function addProject() {
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
      state.status = "";
    } catch (error) {
      setError(error);
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
      setError(error);
    }
  }

  async function removeProject(project: ProjectSummary) {
    if (project.path === state.piProjectPath && state.streaming) {
      state.status = "Stop the current response before removing this project.";
      return;
    }
    try {
      const removingActiveView = project.path === state.activeProjectPath;
      if (project.path === state.piProjectPath) {
        await invoke("stop_pi");
        clearPiConnection();
      }
      removeProjectUiState(project.path);
      state.workspace = await invoke<WorkspaceSnapshot>("remove_project", {
        path: project.path,
      });
      if (removingActiveView) clearActiveSession();
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace.activeProjectPath;
      }
      state.status = "";
    } catch (error) {
      setError(error);
    }
  }

  function newSession(project: ProjectSummary) {
    if (state.startingSession || state.switchingSession) return;
    if (state.streaming || state.stopping) {
      state.status = "Stop the current response before starting a new session.";
      return;
    }

    removeEmptyActivePhantom();
    phantomSequence += 1;
    const now = Date.now();
    const id = `phantom-${now}-${phantomSequence}`;
    const session: EphemeralSession = {
      id,
      path: "",
      title: "New session",
      lastActive: "now",
      archived: false,
      selected: true,
      projectPath: project.path,
      createdAt: now * 1000 + phantomSequence,
      phantom: true,
    };
    state.ephemeralSessions.push(session);
    if (project.collapsed) {
      project.collapsed = false;
      void persistExpandedProject(project.path);
    }
    setActiveSessionView(project, session);
    state.messages = [];
    state.status = "";
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ) {
    const alreadySelected =
      project.path === state.activeProjectPath &&
      session.id === state.activeSessionId;
    if (alreadySelected) {
      markSessionRead(project.path, session.id);
      return;
    }
    if (
      state.streaming ||
      state.stopping ||
      state.startingSession ||
      state.switchingSession
    ) {
      return;
    }

    removeEmptyActivePhantom();
    setActiveSessionView(project, session);
    state.messages = [];
    state.status = "";
    markSessionRead(project.path, session.id);

    if (isPhantomSession(project.path, session.id)) return;

    state.switchingSession = true;
    try {
      if (
        state.piReady &&
        state.piProjectPath === project.path &&
        state.piSessionId === session.id
      ) {
        const id = nextRequestId("selected-state");
        selectionStateRequestId = id;
        await rpc({ id, type: "get_state" });
      } else if (state.piReady && state.piProjectPath === project.path) {
        await rpc({
          id: nextRequestId("switch-session"),
          type: "switch_session",
          sessionPath: session.path,
        });
      } else {
        await startProject(project, session.path);
      }
    } catch (error) {
      state.switchingSession = false;
      setError(error);
    }
  }

  async function sendMessage() {
    const message = state.draft.trim();
    if (!message || !canCompose.value || state.streaming || state.stopping) {
      return;
    }

    if (activeIsPhantom.value) {
      await sendPhantomMessage(message);
      return;
    }

    const key = activeSessionKey();
    state.draft = "";
    if (key) ensureSessionActivity(key).working = true;
    state.messages.push({
      id: `optimistic-user-${Date.now()}`,
      kind: "user",
      text: message,
    });
    state.status = "";
    try {
      await rpc({ id: nextRequestId("prompt"), type: "prompt", message });
    } catch (error) {
      if (key) ensureSessionActivity(key).working = false;
      setError(error);
    }
  }

  async function stop() {
    if (!state.streaming || state.stopping) return;
    state.stopping = true;
    state.status = "";
    try {
      await rpc({ id: nextRequestId("abort"), type: "abort" });
    } catch (error) {
      state.stopping = false;
      setError(error);
    }
  }

  async function selectModel(value: string) {
    const model = state.models.find(
      (option) => `${option.provider}/${option.id}` === value,
    );
    if (!model || settingsDisabled.value) return;
    state.status = "";
    try {
      await rpc({
        id: nextRequestId("set-model"),
        type: "set_model",
        provider: model.provider,
        modelId: model.id,
      });
    } catch (error) {
      setError(error);
    }
  }

  async function selectEffort(level: ThinkingLevel) {
    if (settingsDisabled.value) return;
    state.pendingEffort = level;
    state.status = "";
    try {
      await rpc({
        id: nextRequestId("set-effort"),
        type: "set_thinking_level",
        level,
      });
    } catch (error) {
      state.pendingEffort = "";
      setError(error);
    }
  }

  return {
    state,
    activeProject,
    sessionTitle,
    currentModelLabel,
    currentEffortLabel,
    effortLabels,
    settingsDisabled,
    canCompose,
    initialize,
    dispose,
    addProject,
    toggleProject,
    removeProject,
    newSession,
    selectSession,
    projectSessions,
    isSessionSelected,
    sessionIndicator,
    sendMessage,
    stop,
    selectModel,
    selectEffort,
  };
}

async function sendPhantomMessage(message: string) {
  const project = activeProject.value;
  const phantomId = state.activeSessionId;
  if (!project || !isPhantomSession(project.path, phantomId)) return;

  const optimisticId = `optimistic-user-${Date.now()}`;
  pendingPrompt = {
    projectPath: project.path,
    phantomId,
    message,
    optimisticId,
    stateRequestId: "",
    messagesRequestId: "",
    actualSessionId: "",
  };
  selectionStateRequestId = "";
  state.draft = "";
  ensureSessionActivity(sessionKey(project.path, phantomId)).working = true;
  state.messages.push({ id: optimisticId, kind: "user", text: message });
  state.startingSession = true;
  state.status = "";

  try {
    if (state.piReady && state.piProjectPath === project.path) {
      await rpc({ id: nextRequestId("new-session"), type: "new_session" });
    } else {
      await startProject(project, undefined, true);
    }
  } catch (error) {
    cancelPendingPrompt(error);
  }
}

async function startProject(
  project: ProjectSummary,
  sessionPath?: string,
  preserveMessages = false,
) {
  state.piReady = false;
  state.piProjectPath = project.path;
  state.piSessionId = "";
  state.piSessionPath = "";
  state.piSessionName = "";
  if (!preserveMessages) state.messages = [];
  state.models = [];
  state.efforts = [];
  state.status = "";
  state.workspace = await invoke<WorkspaceSnapshot>("set_active_project", {
    path: project.path,
  });
  state.activeGeneration = await invoke<number>(
    getActivePiIntegration().startCommand,
    {
      projectPath: project.path,
      sessionPath: sessionPath ?? null,
    },
  );
  await requestBootstrap();
}

async function requestBootstrap() {
  const stateRequestId = nextRequestId("state");
  if (pendingPrompt) pendingPrompt.stateRequestId = stateRequestId;
  else selectionStateRequestId = stateRequestId;
  await rpc({ id: stateRequestId, type: "get_state" });
  await rpc({ id: nextRequestId("models"), type: "get_available_models" });
}

async function rpc(request: Record<string, unknown>) {
  await invoke("send_pi", { request });
}

async function handleBridgeEvent(event: PiBridgeEvent) {
  if (event.generation !== state.activeGeneration && event.kind !== "started")
    return;
  if (event.kind === "rpc" && event.line) {
    let value: unknown;
    try {
      value = JSON.parse(event.line);
    } catch {
      return;
    }
    await handleRpc(value);
    return;
  }
  if (event.kind === "stderr") return;
  if (event.kind === "error") {
    const message = event.message || "The Pi connection failed.";
    if (pendingPrompt) cancelPendingPrompt(message);
    else state.status = message;
    return;
  }
  if (event.kind === "exited") {
    const message =
      event.code === 0
        ? ""
        : event.message || "The Pi process stopped unexpectedly.";
    if (pendingPrompt)
      cancelPendingPrompt(message || "The Pi process stopped.");
    const key = connectedSessionKey();
    if (key) ensureSessionActivity(key).working = false;
    state.piReady = false;
    state.streaming = false;
    state.stopping = false;
    state.startingSession = false;
    state.switchingSession = false;
    state.status = message;
  }
}

async function handleRpc(value: unknown) {
  const event = asRecord(value);
  if (!event) return;
  const type = stringValue(event.type);
  if (type === "response") {
    await handleResponse(event);
    return;
  }
  if (type === "agent_start") {
    state.streaming = true;
    state.stopping = false;
    state.status = "";
    const key = connectedSessionKey();
    if (key) ensureSessionActivity(key).working = true;
    return;
  }
  if (type === "message_update") {
    const delta = asRecord(event.assistantMessageEvent);
    const deltaType = stringValue(delta?.type);
    if (deltaType === "text_delta") {
      appendStream("assistant", stringValue(delta?.delta));
      const key = connectedSessionKey();
      if (key) ensureSessionActivity(key).unread = true;
    }
    if (deltaType === "thinking_delta") {
      appendStream("thinking", stringValue(delta?.delta));
    }
    return;
  }
  if (type === "tool_execution_start") {
    const toolCallId = stringValue(event.toolCallId);
    const toolName = stringValue(event.toolName) || "tool";
    state.messages.push({
      id: `stream-tool-${state.streamSequence++}`,
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
    const tool = [...state.messages]
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
    const key = connectedSessionKey();
    if (key) ensureSessionActivity(key).working = false;
    state.streaming = false;
    state.stopping = false;
    state.status = "";
    await rpc({ id: nextRequestId("settled-state"), type: "get_state" });
    return;
  }
  if (type === "auto_retry_start") {
    state.status = `Retrying (${String(event.attempt ?? "")})…`;
    return;
  }
  if (type === "extension_error") {
    state.status = stringValue(event.error) || "A Pi extension failed.";
  }
}

async function handleResponse(response: Record<string, unknown>) {
  const command = stringValue(response.command);
  const responseId = stringValue(response.id);
  if (response.success !== true) {
    const error = asRecord(response.error);
    state.status =
      stringValue(error?.message) ||
      stringValue(response.error) ||
      `Pi rejected ${command || "the request"}.`;
    const failedPendingRequest =
      Boolean(pendingPrompt) &&
      (command === "new_session" ||
        (command === "get_state" &&
          responseId === pendingPrompt?.stateRequestId) ||
        (command === "get_messages" &&
          responseId === pendingPrompt?.messagesRequestId));
    if (failedPendingRequest) cancelPendingPrompt(state.status);

    const key = connectedSessionKey();
    if (command === "prompt" && key) {
      ensureSessionActivity(key).working = false;
    }
    if (
      command === "switch_session" ||
      (command === "get_state" && responseId === selectionStateRequestId)
    ) {
      state.switchingSession = false;
      selectionStateRequestId = "";
    }
    if (command === "abort") state.stopping = false;
    return;
  }
  const data = asRecord(response.data);

  if (command === "get_state" && data) {
    const model = asRecord(data.model);
    state.currentModelProvider = stringValue(model?.provider);
    state.currentModelId = stringValue(model?.id);
    state.currentModelName = stringValue(model?.name);
    state.currentEffort = normalizeEffort(data.thinkingLevel);
    state.piSessionId = stringValue(data.sessionId);
    state.piSessionPath = stringValue(data.sessionFile);
    state.piSessionName = stringValue(data.sessionName);
    state.piReady = true;
    state.streaming = data.isStreaming === true;
    state.stopping = false;
    state.status = "";

    const resolvesPending =
      Boolean(pendingPrompt?.stateRequestId) &&
      responseId === pendingPrompt?.stateRequestId;
    const resolvesSelection =
      Boolean(selectionStateRequestId) &&
      responseId === selectionStateRequestId;
    const updatesCurrentView =
      resolvesPending ||
      resolvesSelection ||
      (!pendingPrompt &&
        !selectionStateRequestId &&
        !activeIsPhantom.value &&
        !state.startingSession &&
        !state.switchingSession);

    if (updatesCurrentView) {
      if (resolvesPending && pendingPrompt) {
        materializePendingSession(
          pendingPrompt,
          state.piSessionId,
          state.piSessionPath,
        );
      } else {
        state.activeProjectPath = state.piProjectPath;
        state.activeSessionId = state.piSessionId;
        state.activeSessionPath = state.piSessionPath;
      }
      state.sessionName = state.piSessionName;
      if (resolvesSelection) selectionStateRequestId = "";
      state.switchingSession = false;
      if (!pendingPrompt) state.startingSession = false;
    }

    const connectedKey = connectedSessionKey();
    if (connectedKey) {
      const activity = ensureSessionActivity(connectedKey);
      if (state.streaming) activity.working = true;
      else if (!pendingPrompt) activity.working = false;
    }

    if (updatesCurrentView && state.piSessionId && state.piSessionPath) {
      const registered = await registerConnectedSession();
      if (registered && !pendingPrompt) removeRegisteredEphemeralSession();
    }

    const messagesRequestId = nextRequestId("messages");
    if (resolvesPending && pendingPrompt) {
      pendingPrompt.messagesRequestId = messagesRequestId;
    }
    await rpc({ id: messagesRequestId, type: "get_messages" });
    await rpc({
      id: nextRequestId("efforts"),
      type: "get_available_thinking_levels",
    });
    return;
  }

  if (command === "get_messages" && data) {
    const resolvesPending =
      Boolean(pendingPrompt?.messagesRequestId) &&
      responseId === pendingPrompt?.messagesRequestId;
    const viewingConnectedSession =
      state.activeProjectPath === state.piProjectPath &&
      state.activeSessionId === state.piSessionId &&
      !activeIsPhantom.value;
    if (!resolvesPending && !viewingConnectedSession) return;

    state.messages = hydrateTranscript(
      Array.isArray(data.messages) ? data.messages : [],
    );
    if (resolvesPending) await dispatchPendingPrompt();
    return;
  }

  if (command === "get_available_models" && data) {
    state.models = (Array.isArray(data.models) ? data.models : [])
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
    state.efforts = (Array.isArray(data.levels) ? data.levels : [])
      .map(normalizeEffort)
      .filter((level, index, levels) => levels.indexOf(level) === index);
    return;
  }

  if (command === "new_session" || command === "switch_session") {
    if (!pendingPrompt) state.messages = [];
    const id = nextRequestId("changed-state");
    if (pendingPrompt) pendingPrompt.stateRequestId = id;
    else selectionStateRequestId = id;
    await rpc({ id, type: "get_state" });
    return;
  }

  if (command === "set_model") {
    await rpc({ id: nextRequestId("model-state"), type: "get_state" });
    return;
  }

  if (command === "set_thinking_level") {
    if (state.pendingEffort) state.currentEffort = state.pendingEffort;
    state.pendingEffort = "";
    state.status = "";
  }
}

async function dispatchPendingPrompt() {
  const prompt = pendingPrompt;
  if (!prompt) return;
  pendingPrompt = undefined;
  state.startingSession = false;
  if (!state.messages.some((message) => message.id === prompt.optimisticId)) {
    state.messages.push({
      id: prompt.optimisticId,
      kind: "user",
      text: prompt.message,
    });
  }
  const key = connectedSessionKey();
  if (key) ensureSessionActivity(key).working = true;
  try {
    await rpc({
      id: nextRequestId("prompt"),
      type: "prompt",
      message: prompt.message,
    });
  } catch (error) {
    if (key) ensureSessionActivity(key).working = false;
    if (
      state.activeProjectPath === prompt.projectPath &&
      state.activeSessionId === prompt.actualSessionId
    ) {
      state.draft = prompt.message;
    }
    setError(error);
  }
}

async function registerConnectedSession(): Promise<boolean> {
  try {
    state.workspace = await invoke<WorkspaceSnapshot>("register_session", {
      projectPath: state.piProjectPath,
      sessionId: state.piSessionId,
      sessionPath: state.piSessionPath,
      sessionName: state.piSessionName || null,
    });
    return true;
  } catch (error) {
    setError(error);
    return false;
  }
}

async function persistExpandedProject(projectPath: string) {
  try {
    state.workspace = await invoke<WorkspaceSnapshot>("set_project_collapsed", {
      path: projectPath,
      collapsed: false,
    });
  } catch (error) {
    setError(error);
  }
}

function materializePendingSession(
  prompt: PendingPrompt,
  sessionId: string,
  sessionPath: string,
) {
  const session = ephemeralSession(prompt.projectPath, prompt.phantomId);
  const previousKey = sessionKey(prompt.projectPath, prompt.phantomId);
  const nextKey = sessionKey(prompt.projectPath, sessionId);
  const activity = ensureSessionActivity(previousKey);

  prompt.actualSessionId = sessionId;
  if (session) {
    session.id = sessionId;
    session.path = sessionPath;
    session.title = draftTitle(prompt.message);
    session.phantom = false;
  }
  sessionDrafts[nextKey] = "";
  sessionActivity[nextKey] = activity;
  delete sessionDrafts[previousKey];
  delete sessionActivity[previousKey];
  state.activeSessionId = sessionId;
  state.activeSessionPath = sessionPath;
}

function cancelPendingPrompt(error: unknown) {
  const prompt = pendingPrompt;
  if (!prompt) {
    setError(error);
    return;
  }
  pendingPrompt = undefined;
  state.startingSession = false;
  const sessionId = prompt.actualSessionId || prompt.phantomId;
  const key = sessionKey(prompt.projectPath, sessionId);
  ensureSessionActivity(key).working = false;
  state.messages = state.messages.filter(
    (message) => message.id !== prompt.optimisticId,
  );
  if (
    state.activeProjectPath === prompt.projectPath &&
    state.activeSessionId === sessionId
  ) {
    state.draft = prompt.message;
  } else {
    sessionDrafts[key] = prompt.message;
  }
  setError(error);
}

function appendStream(kind: "assistant" | "thinking", delta: string) {
  if (!delta) return;
  const last = state.messages[state.messages.length - 1];
  if (last?.kind === kind && last.id.startsWith("stream-")) {
    last.text += delta;
    return;
  }
  state.messages.push({
    id: `stream-${kind}-${state.streamSequence++}`,
    kind,
    text: delta,
  });
}

function projectSessions(project: ProjectSummary): SessionSummary[] {
  const ephemeral = state.ephemeralSessions
    .filter((session) => session.projectPath === project.path)
    .sort((left, right) => right.createdAt - left.createdAt);
  const ephemeralIds = new Set(ephemeral.map((session) => session.id));
  return [
    ...ephemeral,
    ...project.sessions.filter(
      (session) => !session.archived && !ephemeralIds.has(session.id),
    ),
  ];
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
  const key = sessionKey(project.path, session.id);
  const activity = sessionActivity[key];
  if (activity?.working) return "working";
  if (sessionDrafts[key]?.trim()) return "draft";
  return activity?.unread ? "new" : "";
}

function setActiveSessionView(
  project: ProjectSummary,
  session: SessionSummary,
) {
  state.activeProjectPath = project.path;
  state.activeSessionId = session.id;
  state.activeSessionPath = session.path;
  state.sessionName = isPhantomSession(project.path, session.id)
    ? ""
    : session.title === "New session"
      ? ""
      : session.title;
  state.draft = sessionDrafts[sessionKey(project.path, session.id)] || "";
}

function markSessionRead(projectPath: string, sessionId: string) {
  const activity = sessionActivity[sessionKey(projectPath, sessionId)];
  if (activity) activity.unread = false;
}

function removeEmptyActivePhantom() {
  const session = ephemeralSession(
    state.activeProjectPath,
    state.activeSessionId,
  );
  if (!session?.phantom || state.draft.trim()) return;
  removeEphemeralSession(session);
}

function removeRegisteredEphemeralSession() {
  const session = state.ephemeralSessions.find(
    (item) =>
      !item.phantom &&
      item.projectPath === state.piProjectPath &&
      item.id === state.piSessionId,
  );
  if (session) removeEphemeralSession(session, false);
}

function removeEphemeralSession(
  session: EphemeralSession,
  removeUiState = true,
) {
  const index = state.ephemeralSessions.indexOf(session);
  if (index >= 0) state.ephemeralSessions.splice(index, 1);
  if (removeUiState) {
    const key = sessionKey(session.projectPath, session.id);
    delete sessionDrafts[key];
    delete sessionActivity[key];
  }
}

function removeProjectUiState(projectPath: string) {
  state.ephemeralSessions = state.ephemeralSessions.filter(
    (session) => session.projectPath !== projectPath,
  );
  const prefix = `${projectPath}\u0000`;
  for (const key of Object.keys(sessionDrafts)) {
    if (key.startsWith(prefix)) delete sessionDrafts[key];
  }
  for (const key of Object.keys(sessionActivity)) {
    if (key.startsWith(prefix)) delete sessionActivity[key];
  }
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

function isPhantomSession(projectPath: string, sessionId: string): boolean {
  return ephemeralSession(projectPath, sessionId)?.phantom === true;
}

function ensureSessionActivity(key: string): SessionActivity {
  return (sessionActivity[key] ??= { unread: false, working: false });
}

function activeSessionKey(): string {
  return state.activeProjectPath && state.activeSessionId
    ? sessionKey(state.activeProjectPath, state.activeSessionId)
    : "";
}

function connectedSessionKey(): string {
  return state.piProjectPath && state.piSessionId
    ? sessionKey(state.piProjectPath, state.piSessionId)
    : "";
}

function sessionKey(projectPath: string, sessionId: string): string {
  return `${projectPath}\u0000${sessionId}`;
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

function firstUserMessage(): string {
  return state.messages.find((message) => message.kind === "user")?.text || "";
}

function clearActiveSession() {
  state.messages = [];
  state.draft = "";
  state.activeProjectPath = "";
  state.activeSessionId = "";
  state.activeSessionPath = "";
  state.sessionName = "";
}

function clearPiConnection() {
  const key = connectedSessionKey();
  if (key) ensureSessionActivity(key).working = false;
  state.piReady = false;
  state.streaming = false;
  state.stopping = false;
  state.piProjectPath = "";
  state.piSessionId = "";
  state.piSessionPath = "";
  state.piSessionName = "";
}

function setError(error: unknown) {
  state.status = error instanceof Error ? error.message : String(error);
}
