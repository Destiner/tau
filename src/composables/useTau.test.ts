import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import type {
  PiBridgeEvent,
  ProjectSummary,
  WorkspaceSnapshot,
} from "../types";
import { useTau } from "./useTau";

const mocks = vi.hoisted(() => ({
  workspace: null as WorkspaceSnapshot | null,
  generation: 0,
  listener: undefined as
    ((event: { payload: PiBridgeEvent }) => void) | undefined,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command.startsWith("start_pi")) {
      mocks.generation += 1;
      return mocks.generation;
    }
    return mocks.workspace;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (
      _event: string,
      listener: (event: { payload: PiBridgeEvent }) => void,
    ) => {
      mocks.listener = listener;
      return vi.fn();
    },
  ),
}));

describe("session drafts and selection", () => {
  it("switches away from a working session while preserving its state", async () => {
    const project: ProjectSummary = {
      path: "/tmp/tau-draft-test",
      name: "tau-draft-test",
      workingDirectory: "/tmp/tau-draft-test",
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    const {
      state,
      draft,
      newSession,
      selectSession,
      projectSessions,
      sessionIndicator,
    } = useTau();

    state.activeProjectPath = "";
    state.activeSessionId = "";
    state.activeSessionPath = "";
    state.activeControllerKey = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await newSession(project);
    const first = projectSessions(project)[0];
    expect(first?.title).toBe("New session");

    draft.value = "  Keep this draft\nwith its session  ";
    expect(first?.title).toBe("Keep this draft with its session");
    expect(first && sessionIndicator(project, first)).toBe("draft");

    const firstController = state.controllers.find(
      (controller) => controller.sessionId === first?.id,
    );
    if (!first || !firstController) {
      throw new Error("Expected the first phantom session");
    }
    firstController.streaming = true;
    firstController.working = true;

    await newSession(project);
    expect(projectSessions(project)).toHaveLength(2);
    expect(sessionIndicator(project, first)).toBe("working");

    const empty = projectSessions(project)[0];
    expect(empty?.title).toBe("New session");
    await selectSession(project, first);

    expect(projectSessions(project)).toEqual([first]);
    expect(draft.value).toBe("  Keep this draft\nwith its session  ");
    expect(state.activeSessionId).toBe(first.id);
    expect(firstController.streaming).toBe(true);
  });

  it("starts a second runtime without stopping a working session", async () => {
    const firstSession = savedSession("first");
    const secondSession = savedSession("second");
    const project: ProjectSummary = {
      path: "/tmp/tau-concurrency-test",
      name: "tau-concurrency-test",
      workingDirectory: "/tmp/tau-concurrency-test",
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const { state, initialize, selectSession, sessionIndicator } = useTau();
    await initialize();
    state.activeProjectPath = "";
    state.activeSessionId = "";
    state.activeSessionPath = "";
    state.activeControllerKey = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, firstSession);
    const firstController = state.controllers.find(
      (controller) => controller.sessionId === firstSession.id,
    );
    if (!firstController) throw new Error("Expected the first controller");
    firstController.starting = false;
    firstController.ready = true;
    firstController.streaming = true;
    firstController.working = true;

    await selectSession(project, secondSession);

    const startCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "start_pi");
    expect(startCalls).toHaveLength(2);
    expect(startCalls[0]?.[1]).not.toEqual(startCalls[1]?.[1]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === "stop_pi" &&
            (args as { runtimeId?: string })?.runtimeId ===
              firstController.runtimeId,
        ),
    ).toBe(false);
    expect(firstController.streaming).toBe(true);
    expect(state.activeSessionId).toBe(secondSession.id);

    const secondController = state.controllers.find(
      (controller) => controller.sessionId === secondSession.id,
    );
    if (!secondController || !mocks.listener) {
      throw new Error("Expected the second controller and event listener");
    }
    emitTextDelta(firstController, "background");
    emitTextDelta(secondController, "foreground");
    await vi.waitFor(() => {
      expect(
        firstController.messages[firstController.messages.length - 1]?.text,
      ).toBe("background");
      expect(
        secondController.messages[secondController.messages.length - 1]?.text,
      ).toBe("foreground");
    });
    expect(firstController.unread).toBe(true);
    expect(secondController.unread).toBe(false);

    secondController.unread = true;
    expect(sessionIndicator(project, secondSession)).toBe("");
  });

  it("keeps a submitted new session visible until its file is listed", async () => {
    const project: ProjectSummary = {
      path: "/tmp/tau-materialization-test",
      name: "tau-materialization-test",
      workingDirectory: "/tmp/tau-materialization-test",
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    const {
      state,
      currentEffortLabel,
      currentModelLabel,
      draft,
      efforts,
      initialize,
      models,
      newSession,
      projectSessions,
      sendMessage,
      settingsDisabled,
    } = useTau();
    await initialize();
    state.activeControllerKey = "";
    state.activeSessionId = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await newSession(project);
    const controller = state.controllers[0];
    if (!controller) throw new Error("Expected a pending controller");

    const modelsRequest = sentRequests(controller, "get_available_models")[0];
    emitRpc(controller, {
      id: modelsRequest?.id,
      type: "response",
      command: "get_available_models",
      success: true,
      data: {
        models: [
          {
            provider: "provider",
            id: "alpha",
            name: "Alpha",
            reasoning: true,
          },
        ],
      },
    });
    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: { provider: "provider", id: "alpha", name: "Alpha" },
        thinkingLevel: "high",
        sessionId: "provisional",
        sessionFile: "/tmp/provisional.jsonl",
        sessionName: "",
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.startMessagesRequestId).not.toBe("");
      expect(
        sentRequests(controller, "get_available_thinking_levels"),
      ).toHaveLength(1);
    });
    const effortsRequest = sentRequests(
      controller,
      "get_available_thinking_levels",
    )[0];
    emitRpc(controller, {
      id: effortsRequest?.id,
      type: "response",
      command: "get_available_thinking_levels",
      success: true,
      data: { levels: ["off", "high"] },
    });
    emitRpc(controller, {
      id: controller.startMessagesRequestId,
      type: "response",
      command: "get_messages",
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(controller.starting).toBe(false);
    });
    expect(controller.sessionId).toMatch(/^phantom-/);
    expect(models.value).toHaveLength(1);
    expect(efforts.value).toEqual(["off", "high"]);
    expect(currentModelLabel.value).toBe("Alpha");
    expect(currentEffortLabel.value).toBe("High");
    expect(settingsDisabled.value).toBe(false);

    draft.value = "Start background work";
    await sendMessage();
    emitRpc(controller, {
      id: controller.pendingPrompt?.stateRequestId,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: { provider: "provider", id: "alpha", name: "Alpha" },
        thinkingLevel: "high",
        sessionId: "materialized",
        sessionFile: "/tmp/materialized.jsonl",
        sessionName: "",
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.sessionId).toBe("materialized");
      expect(controller.pendingPrompt?.messagesRequestId).not.toBe("");
    });
    emitRpc(controller, {
      id: controller.pendingPrompt?.messagesRequestId,
      type: "response",
      command: "get_messages",
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(controller.pendingPrompt).toBeUndefined();
    });

    await newSession(project);

    expect(
      projectSessions(project).some((session) => session.id === "materialized"),
    ).toBe(true);
  });

  it("keeps a fresh session at the top and applies inherited settings", async () => {
    const saved = savedSession("saved", 10_000);
    const project: ProjectSummary = {
      path: "/tmp/tau-new-session-settings-test",
      name: "tau-new-session-settings-test",
      workingDirectory: "/tmp/tau-new-session-settings-test",
      collapsed: false,
      selected: true,
      sessions: [saved],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const {
      state,
      currentEffortLabel,
      currentModelId,
      currentModelLabel,
      draft,
      initialize,
      models,
      newSession,
      projectSessions,
      selectEffort,
      selectModel,
      selectSession,
      sendMessage,
      settingsDisabled,
    } = useTau();
    await initialize();
    state.activeControllerKey = "";
    state.activeSessionId = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, saved);
    const savedController = state.controllers[0];
    if (!savedController) throw new Error("Expected the saved controller");
    savedController.starting = false;
    savedController.ready = true;
    savedController.models = [
      {
        provider: "provider",
        id: "alpha",
        name: "Alpha",
        reasoning: true,
      },
      {
        provider: "provider",
        id: "beta",
        name: "Beta",
        reasoning: true,
      },
    ];
    savedController.efforts = ["off", "high", "max"];
    savedController.currentModelProvider = "provider";
    savedController.currentModelId = "alpha";
    savedController.currentModelName = "Alpha";
    savedController.currentEffort = "high";

    await newSession(project);

    expect(projectSessions(project)[0]?.title).toBe("New session");
    expect(models.value).toEqual(savedController.models);
    expect(currentModelLabel.value).toBe("Alpha");
    expect(currentEffortLabel.value).toBe("High");
    expect(settingsDisabled.value).toBe(false);

    await selectModel("provider/beta");
    await selectEffort("max");
    expect(currentModelId.value).toBe("beta");
    expect(currentModelLabel.value).toBe("Beta");
    expect(currentEffortLabel.value).toBe("Max");

    draft.value = "Use these settings";
    await sendMessage();
    const controller = state.controllers.find(
      (candidate) => candidate !== savedController,
    );
    if (!controller) throw new Error("Expected the new controller");

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: { provider: "provider", id: "alpha", name: "Alpha" },
        thinkingLevel: "off",
        sessionId: "new-session",
        sessionFile: "/tmp/new-session.jsonl",
        sessionName: "",
        isStreaming: false,
      },
    });

    await vi.waitFor(() => {
      expect(sentRequests(controller, "set_model")).toHaveLength(1);
    });
    const modelRequest = sentRequests(controller, "set_model")[0];
    expect(modelRequest).toMatchObject({
      provider: "provider",
      modelId: "beta",
    });
    expect(sentRequests(controller, "prompt")).toHaveLength(0);

    emitRpc(controller, {
      id: modelRequest?.id,
      type: "response",
      command: "set_model",
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, "set_thinking_level")).toHaveLength(1);
    });
    const effortRequest = sentRequests(controller, "set_thinking_level")[0];
    expect(effortRequest).toMatchObject({ level: "max" });
    expect(sentRequests(controller, "prompt")).toHaveLength(0);

    emitRpc(controller, {
      id: effortRequest?.id,
      type: "response",
      command: "set_thinking_level",
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, "get_messages")).toHaveLength(1);
    });
    const messagesRequest = sentRequests(controller, "get_messages")[0];
    emitRpc(controller, {
      id: messagesRequest?.id,
      type: "response",
      command: "get_messages",
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, "prompt")).toHaveLength(1);
    });
  });

  it("archives a session without stopping its running controller", async () => {
    const firstSession = savedSession("first", 2_000);
    const secondSession = savedSession("second", 1_000);
    const project: ProjectSummary = {
      path: "/tmp/tau-archive-test",
      name: "tau-archive-test",
      workingDirectory: "/tmp/tau-archive-test",
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const { archiveSession, projectSessions, selectSession, state } = useTau();
    state.activeProjectPath = "";
    state.activeSessionId = "";
    state.activeSessionPath = "";
    state.activeControllerKey = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, firstSession);
    const firstController = state.controllers.find(
      (controller) => controller.sessionId === firstSession.id,
    );
    if (!firstController) throw new Error("Expected the first controller");
    firstController.starting = false;
    firstController.ready = true;
    firstController.streaming = true;
    firstController.working = true;

    const archivedProject: ProjectSummary = {
      ...project,
      sessions: [
        { ...firstSession, archived: true, selected: false },
        secondSession,
      ],
    };
    mocks.workspace = { ...workspace, projects: [archivedProject] };

    await archiveSession(project, firstSession);

    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === "archive_session" &&
            (args as { projectPath?: string; sessionId?: string })
              ?.projectPath === project.path &&
            (args as { projectPath?: string; sessionId?: string })
              ?.sessionId === firstSession.id,
        ),
    ).toBe(true);
    expect(projectSessions(archivedProject)).toEqual([secondSession]);
    expect(
      archivedProject.sessions.filter((session) => session.archived),
    ).toEqual([{ ...firstSession, archived: true, selected: false }]);
    expect(state.activeSessionId).toBe(secondSession.id);
    expect(firstController.streaming).toBe(true);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === "stop_pi" &&
            (args as { runtimeId?: string })?.runtimeId ===
              firstController.runtimeId,
        ),
    ).toBe(false);
  });

  it("reorders sessions only when the user submits a message", async () => {
    const firstSession = savedSession("first", 1_000);
    const secondSession = savedSession("second", 2_000);
    const project: ProjectSummary = {
      path: "/tmp/tau-order-test",
      name: "tau-order-test",
      workingDirectory: "/tmp/tau-order-test",
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    mocks.workspace = workspace;
    const {
      state,
      canCompose,
      canDraft,
      draft,
      initialize,
      projectSessions,
      selectSession,
      sendMessage,
    } = useTau();
    await initialize();
    state.activeControllerKey = "";
    state.activeSessionId = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    expect(projectSessions(project)[0]?.id).toBe("second");
    await selectSession(project, firstSession);
    expect(canDraft.value).toBe(true);
    expect(canCompose.value).toBe(false);
    draft.value = "Move this session to the top";
    const firstController = state.controllers[0];
    if (!firstController) throw new Error("Expected the first controller");
    firstController.starting = false;
    firstController.ready = true;
    await sendMessage();
    expect(projectSessions(project)[0]?.id).toBe("first");

    await selectSession(project, secondSession);
    const secondController = state.controllers.find(
      (controller) => controller.sessionId === secondSession.id,
    );
    if (!secondController) throw new Error("Expected the second controller");
    emitTextDelta(secondController, "Assistant-only update");
    await vi.waitFor(() => {
      expect(secondController.messages).toHaveLength(1);
    });

    expect(projectSessions(project)[0]?.id).toBe("first");
  });
});

function emitRpc(
  controller: { runtimeId: string; generation: number },
  value: unknown,
) {
  mocks.listener?.({
    payload: {
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: "rpc",
      line: JSON.stringify(value),
    },
  });
}

function emitTextDelta(
  controller: { runtimeId: string; generation: number },
  delta: string,
) {
  mocks.listener?.({
    payload: {
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: "rpc",
      line: JSON.stringify({
        type: "message_update",
        assistantMessageEvent: { type: "text_delta", delta },
      }),
    },
  });
}

function sentRequests(
  controller: { runtimeId: string },
  type: string,
): Array<Record<string, unknown>> {
  return vi
    .mocked(invoke)
    .mock.calls.filter(
      ([command, args]) =>
        command === "send_pi" &&
        (args as { runtimeId?: string })?.runtimeId === controller.runtimeId,
    )
    .map(([, args]) => (args as { request: Record<string, unknown> }).request)
    .filter((request) => request.type === type);
}

function savedSession(id: string, lastUserMessageAt = 0) {
  return {
    id,
    path: `/tmp/${id}.jsonl`,
    title: id,
    lastActive: "now",
    lastUserMessageAt,
    archived: false,
    selected: false,
  };
}
