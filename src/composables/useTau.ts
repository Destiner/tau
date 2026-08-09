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
  currentModelProvider: "",
  currentModelId: "",
  currentModelName: "",
  currentEffort: "off" as ThinkingLevel,
  pendingEffort: "" as ThinkingLevel | "",
  models: [] as ModelOption[],
  efforts: [] as ThinkingLevel[],
  requestSequence: 0,
  streamSequence: 0,
});

let unlisten: UnlistenFn | undefined;

const activeProject = computed(() =>
  state.workspace?.projects.find(
    (project) => project.path === state.activeProjectPath,
  ),
);

const activeSession = computed(() =>
  activeProject.value?.sessions.find(
    (session) => session.id === state.activeSessionId,
  ),
);

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
    state.streaming ||
    state.stopping ||
    state.startingSession ||
    state.switchingSession,
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
        await startProject(selectedProject, selectedSession.path);
      }
    } catch (error) {
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
      state.activeProjectPath = state.workspace.activeProjectPath;
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
    if (project.path === state.activeProjectPath && state.streaming) {
      state.status = "Stop the current response before removing this project.";
      return;
    }
    try {
      if (project.path === state.activeProjectPath) {
        await invoke("stop_pi");
        clearSession();
      }
      state.workspace = await invoke<WorkspaceSnapshot>("remove_project", {
        path: project.path,
      });
      state.activeProjectPath = state.workspace.activeProjectPath;
      state.status = "";
    } catch (error) {
      setError(error);
    }
  }

  async function newSession(project: ProjectSummary) {
    if (state.startingSession || state.switchingSession) return;
    if (state.streaming || state.stopping) {
      state.status = "Stop the current response before starting a new session.";
      return;
    }
    state.messages = [];
    state.sessionName = "";
    state.activeSessionId = "";
    state.activeSessionPath = "";
    state.startingSession = true;
    state.status = "";
    try {
      if (state.piReady && state.activeProjectPath === project.path) {
        await rpc({ id: nextRequestId("new-session"), type: "new_session" });
      } else {
        await startProject(project);
      }
    } catch (error) {
      state.startingSession = false;
      setError(error);
    }
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ) {
    if (
      session.id === state.activeSessionId ||
      state.streaming ||
      state.stopping ||
      state.switchingSession
    ) {
      return;
    }
    state.switchingSession = true;
    state.status = "";
    try {
      if (state.piReady && state.activeProjectPath === project.path) {
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
    if (!message || !state.piReady || state.streaming || state.stopping) return;
    state.draft = "";
    state.messages.push({
      id: `optimistic-user-${Date.now()}`,
      kind: "user",
      text: message,
    });
    state.status = "";
    try {
      await rpc({ id: nextRequestId("prompt"), type: "prompt", message });
    } catch (error) {
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
    initialize,
    dispose,
    addProject,
    toggleProject,
    removeProject,
    newSession,
    selectSession,
    sendMessage,
    stop,
    selectModel,
    selectEffort,
  };
}

async function startProject(project: ProjectSummary, sessionPath?: string) {
  state.piReady = false;
  state.activeProjectPath = project.path;
  state.activeSessionPath = sessionPath ?? "";
  state.messages = [];
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
  await rpc({ id: nextRequestId("state"), type: "get_state" });
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
  if (event.kind === "stderr") {
    return;
  }
  if (event.kind === "error") {
    state.status = event.message || "The Pi connection failed.";
    return;
  }
  if (event.kind === "exited") {
    state.piReady = false;
    state.streaming = false;
    state.stopping = false;
    state.status =
      event.code === 0
        ? ""
        : event.message || "The Pi process stopped unexpectedly.";
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
    return;
  }
  if (type === "message_update") {
    const delta = asRecord(event.assistantMessageEvent);
    const deltaType = stringValue(delta?.type);
    if (deltaType === "text_delta")
      appendStream("assistant", stringValue(delta?.delta));
    if (deltaType === "thinking_delta")
      appendStream("thinking", stringValue(delta?.delta));
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
  if (response.success !== true) {
    const error = asRecord(response.error);
    state.status =
      stringValue(error?.message) || `Pi rejected ${command || "the request"}.`;
    state.startingSession = false;
    state.switchingSession = false;
    state.stopping = false;
    return;
  }
  const data = asRecord(response.data);

  if (command === "get_state" && data) {
    const model = asRecord(data.model);
    state.currentModelProvider = stringValue(model?.provider);
    state.currentModelId = stringValue(model?.id);
    state.currentModelName = stringValue(model?.name);
    state.currentEffort = normalizeEffort(data.thinkingLevel);
    state.activeSessionId = stringValue(data.sessionId);
    state.activeSessionPath = stringValue(data.sessionFile);
    state.sessionName = stringValue(data.sessionName);
    state.piReady = true;
    state.streaming = data.isStreaming === true;
    state.startingSession = false;
    state.switchingSession = false;
    state.status = "";
    if (state.activeSessionId && state.activeSessionPath) {
      void registerActiveSession();
    }
    await rpc({ id: nextRequestId("messages"), type: "get_messages" });
    await rpc({
      id: nextRequestId("efforts"),
      type: "get_available_thinking_levels",
    });
    return;
  }

  if (command === "get_messages" && data) {
    state.messages = hydrateTranscript(
      Array.isArray(data.messages) ? data.messages : [],
    );
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
    state.messages = [];
    await rpc({ id: nextRequestId("changed-state"), type: "get_state" });
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

async function registerActiveSession() {
  try {
    state.workspace = await invoke<WorkspaceSnapshot>("register_session", {
      projectPath: state.activeProjectPath,
      sessionId: state.activeSessionId,
      sessionPath: state.activeSessionPath,
      sessionName: state.sessionName || null,
    });
  } catch (error) {
    setError(error);
  }
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

function clearSession() {
  state.messages = [];
  state.piReady = false;
  state.streaming = false;
  state.stopping = false;
  state.activeProjectPath = "";
  state.activeSessionId = "";
  state.activeSessionPath = "";
  state.sessionName = "";
}

function setError(error: unknown) {
  state.status = error instanceof Error ? error.message : String(error);
}
