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

    const { state, initialize, selectSession } = useTau();
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
      draft,
      initialize,
      newSession,
      projectSessions,
      sendMessage,
    } = useTau();
    await initialize();
    state.activeControllerKey = "";
    state.activeSessionId = "";
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await newSession(project);
    draft.value = "Start background work";
    await sendMessage();
    const controller = state.controllers[0];
    if (!controller) throw new Error("Expected a pending controller");

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: null,
        thinkingLevel: "off",
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
