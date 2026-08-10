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
      commands,
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
    const commandsRequest = sentRequests(controller, "get_commands")[0];
    emitRpc(controller, {
      id: commandsRequest?.id,
      type: "response",
      command: "get_commands",
      success: true,
      data: {
        commands: [
          {
            name: "session-name",
            description: "Name this session",
            source: "extension",
          },
          { name: "skill:review", source: "skill" },
          { name: "ignored", source: "unknown" },
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
    expect(commands.value).toEqual([
      {
        name: "session-name",
        description: "Name this session",
        source: "extension",
      },
      { name: "skill:review", source: "skill" },
    ]);
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
    savedController.commands = [{ name: "session-name", source: "extension" }];
    savedController.commandsLoaded = true;
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

describe("session replacement hardening", () => {
  it("rebinds a background workflow controller without stealing the selected session", async () => {
    const {
      firstController,
      firstSession,
      secondController,
      secondSession,
      project,
      tau,
    } = await setupExtensionControllers();
    firstController.messages.push({
      id: "old-phase-message",
      kind: "assistant",
      text: "Old phase",
    });

    emitRpc(firstController, { type: "agent_start" });
    await vi.waitFor(() => {
      expect(sentRequests(firstController, "get_state")).toHaveLength(1);
    });
    const runStateRequest = sentRequests(firstController, "get_state")[0];
    const replacementSession = {
      ...savedSession("plan-phase"),
      title: "Plan phase",
    };
    const workspace = mocks.workspace;
    if (!workspace) throw new Error("Expected the extension workspace");
    mocks.workspace = {
      ...workspace,
      projects: [
        {
          ...project,
          sessions: [
            firstSession,
            { ...secondSession, selected: true },
            replacementSession,
          ],
        },
      ],
    };

    emitRpc(firstController, {
      id: runStateRequest?.id,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: { provider: "provider", id: "beta", name: "Beta" },
        thinkingLevel: "xhigh",
        sessionId: replacementSession.id,
        sessionFile: replacementSession.path,
        sessionName: replacementSession.title,
        isStreaming: true,
      },
    });

    await vi.waitFor(() => {
      expect(firstController.sessionId).toBe(replacementSession.id);
      expect(
        sentRequests(firstController, "get_available_models"),
      ).toHaveLength(1);
      expect(sentRequests(firstController, "get_commands")).toHaveLength(1);
      expect(
        sentRequests(firstController, "get_available_thinking_levels"),
      ).toHaveLength(1);
      expect(sentRequests(firstController, "get_messages")).toHaveLength(1);
    });

    expect(firstController.key).not.toBe(secondController.key);
    expect(firstController.messages).toEqual([]);
    expect(firstController.commandsLoaded).toBe(false);
    expect(tau.state.activeControllerKey).toBe(secondController.key);
    expect(tau.state.activeSessionId).toBe(secondSession.id);
    expect(
      tau.state.workspace?.projects[0]?.sessions.map((session) => session.id),
    ).toEqual([firstSession.id, secondSession.id, replacementSession.id]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === "register_session" &&
            (args as { sessionId?: string })?.sessionId ===
              replacementSession.id,
        ),
    ).toBe(true);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === "set_active_session"),
    ).toBe(false);

    const commandsRequest = sentRequests(firstController, "get_commands")[0];
    emitRpc(firstController, {
      id: commandsRequest?.id,
      type: "response",
      command: "get_commands",
      success: true,
      data: {
        commands: [
          {
            name: "workflow-next",
            description: "Continue the workflow",
            source: "extension",
          },
        ],
      },
    });
    const messagesRequest = sentRequests(firstController, "get_messages")[0];
    emitRpc(firstController, {
      id: messagesRequest?.id,
      type: "response",
      command: "get_messages",
      success: true,
      data: {
        messages: [{ role: "user", content: "Review the plan" }],
      },
    });
    await vi.waitFor(() => {
      expect(firstController.commands).toEqual([
        {
          name: "workflow-next",
          description: "Continue the workflow",
          source: "extension",
        },
      ]);
      expect(firstController.messages).toEqual([
        { id: "user-0", kind: "user", text: "Review the plan" },
      ]);
    });

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "replacement-approval",
      method: "confirm",
      title: "Approve the plan?",
      message: "The replacement session is waiting.",
    });
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    const replacementProject = tau.state.workspace?.projects[0];
    const replacementView = replacementProject?.sessions.find(
      (session) => session.id === replacementSession.id,
    );
    if (!replacementProject || !replacementView) {
      throw new Error("Expected the registered replacement session");
    }

    await tau.selectSession(replacementProject, replacementView);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      controllerKey: firstController.key,
      runtimeId: firstController.runtimeId,
      sessionName: "Plan phase",
    });
    await tau.submitExtensionDialog(true);
    expect(sentRequests(firstController, "extension_ui_response")).toEqual([
      {
        type: "extension_ui_response",
        id: "replacement-approval",
        confirmed: true,
      },
    ]);
    tau.dispose();
  });

  it("uses an identity-only state request for an unchanged agent run", async () => {
    const { secondController, secondSession, tau } =
      await setupExtensionControllers();

    emitRpc(secondController, { type: "agent_start" });
    await vi.waitFor(() => {
      expect(sentRequests(secondController, "get_state")).toHaveLength(1);
    });
    const runStateRequest = sentRequests(secondController, "get_state")[0];
    emitRpc(secondController, {
      id: runStateRequest?.id,
      type: "response",
      command: "get_state",
      success: true,
      data: {
        model: { provider: "provider", id: "alpha", name: "Alpha" },
        thinkingLevel: "high",
        sessionId: secondSession.id,
        sessionFile: secondSession.path,
        sessionName: secondSession.title,
        isStreaming: true,
      },
    });
    await vi.waitFor(() => {
      expect(secondController.runStateRequestId).toBe("");
    });

    expect(sentRequests(secondController, "get_available_models")).toEqual([]);
    expect(sentRequests(secondController, "get_commands")).toEqual([]);
    expect(
      sentRequests(secondController, "get_available_thinking_levels"),
    ).toEqual([]);
    expect(sentRequests(secondController, "get_messages")).toEqual([]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === "register_session"),
    ).toBe(false);
    tau.dispose();
  });
});

describe("extension UI protocol", () => {
  it("keeps dialogs in their originating session while users switch freely", async () => {
    const {
      firstController,
      firstSession,
      secondController,
      secondSession,
      project,
      tau,
    } = await setupExtensionControllers();

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "reviewer-1",
      method: "select",
      title: "Choose a reviewer",
      options: ["[ ] Claude", "Done"],
    });
    emitRpc(secondController, {
      type: "extension_ui_request",
      id: "merge-1",
      method: "confirm",
      title: "Merge the pull request?",
      message: "This requires manual approval.",
    });

    expect(tau.state.extensionDialogs).toHaveLength(2);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      requestId: "merge-1",
      method: "confirm",
      sessionName: "second",
    });

    await tau.selectSession(project, firstSession);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      requestId: "reviewer-1",
      method: "select",
      projectName: "extension-ui-test",
      sessionName: "first",
    });
    await tau.submitExtensionDialog("[ ] Claude");
    expect(sentRequests(firstController, "extension_ui_response")).toEqual([
      {
        type: "extension_ui_response",
        id: "reviewer-1",
        value: "[ ] Claude",
      },
    ]);

    await tau.selectSession(project, secondSession);
    expect(tau.activeExtensionDialog.value?.requestId).toBe("merge-1");
    await tau.submitExtensionDialog(false);
    expect(sentRequests(secondController, "extension_ui_response")).toEqual([
      {
        type: "extension_ui_response",
        id: "merge-1",
        confirmed: false,
      },
    ]);
    expect(tau.activeExtensionDialog.value).toBeUndefined();

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "reviewer-2",
      method: "select",
      title: "Choose a reviewer",
      options: ["[x] Claude", "Done"],
    });
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    await tau.selectSession(project, firstSession);
    await tau.submitExtensionDialog("Done");

    expect(sentRequests(firstController, "extension_ui_response")).toEqual([
      {
        type: "extension_ui_response",
        id: "reviewer-1",
        value: "[ ] Claude",
      },
      {
        type: "extension_ui_response",
        id: "reviewer-2",
        value: "Done",
      },
    ]);
    tau.dispose();
  });

  it("returns input values and explicit cancellations with the RPC shapes", async () => {
    const { firstController, firstSession, secondSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "issue-id",
      method: "input",
      title: "Linear issue ID",
      placeholder: "ENG-123",
    });
    if (!tau.activeExtensionDialog.value) {
      throw new Error("Expected the input prompt");
    }
    tau.activeExtensionDialog.value.draft = "ENG-42";
    await tau.selectSession(project, secondSession);
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    await tau.selectSession(project, firstSession);
    expect(tau.activeExtensionDialog.value?.draft).toBe("ENG-42");
    await tau.submitExtensionDialog(
      tau.activeExtensionDialog.value?.draft ?? "",
    );

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "task-notes",
      method: "editor",
      title: "Task notes",
      prefill: "Keep the API stable.",
    });
    await tau.cancelExtensionDialog();

    expect(sentRequests(firstController, "extension_ui_response")).toEqual([
      {
        type: "extension_ui_response",
        id: "issue-id",
        value: "ENG-42",
      },
      {
        type: "extension_ui_response",
        id: "task-notes",
        cancelled: true,
      },
    ]);
    tau.dispose();
  });

  it("keeps extension drafts on their hidden session and ignores TUI statuses", async () => {
    const { firstController, firstSession, project, tau } =
      await setupExtensionControllers();
    firstController.status = "Tau connection warning";

    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "editor-text-1",
      method: "set_editor_text",
      text: "/implement",
    });
    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "status-1",
      method: "setStatus",
      statusKey: "mcp",
      statusText: "\u001b[38;2;138;190;183mMCP ready\u001b[39m",
    });
    emitRpc(firstController, {
      type: "extension_ui_request",
      id: "notify-1",
      method: "notify",
      message: "Approval is ready",
      notifyType: "warning",
    });

    expect(firstController.draft).toBe("/implement");
    expect(firstController.status).toBe("Tau connection warning");
    expect(tau.extensionNotifications.value).toEqual([
      expect.objectContaining({
        message: "Approval is ready",
        type: "warning",
        projectName: "extension-ui-test",
        sessionName: "first",
      }),
    ]);

    await tau.selectSession(project, firstSession);

    expect(tau.draft.value).toBe("/implement");
    expect(tau.status.value).toBe("");
    tau.dispose();
  });

  it("discards dialogs after their timeout, generation change, or process exit", async () => {
    const { firstController, firstSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);
    vi.useFakeTimers();

    try {
      emitRpc(firstController, {
        type: "extension_ui_request",
        id: "timed-1",
        method: "input",
        title: "Optional issue ID",
        timeout: 25,
      });
      expect(tau.activeExtensionDialog.value?.requestId).toBe("timed-1");

      vi.advanceTimersByTime(25);
      expect(tau.activeExtensionDialog.value).toBeUndefined();
      expect(sentRequests(firstController, "extension_ui_response")).toEqual(
        [],
      );

      emitRpc(firstController, {
        type: "extension_ui_request",
        id: "old-generation",
        method: "editor",
        title: "Task notes",
        prefill: "Initial notes",
      });
      const nextGeneration = firstController.generation + 1;
      emitBridge(firstController.runtimeId, nextGeneration, "started");
      expect(tau.activeExtensionDialog.value).toBeUndefined();
      expect(firstController.generation).toBe(nextGeneration);

      emitRpc(firstController, {
        type: "extension_ui_request",
        id: "exiting",
        method: "confirm",
        title: "Continue?",
        message: "The process is about to exit.",
      });
      expect(tau.activeExtensionDialog.value?.requestId).toBe("exiting");
      emitBridge(firstController.runtimeId, nextGeneration, "exited", 0);
      expect(tau.activeExtensionDialog.value).toBeUndefined();
    } finally {
      vi.useRealTimers();
      tau.dispose();
    }
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

function emitBridge(
  runtimeId: string,
  generation: number,
  kind: PiBridgeEvent["kind"],
  code?: number,
) {
  mocks.listener?.({
    payload: { runtimeId, generation, kind, code },
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

async function setupExtensionControllers() {
  const firstSession = savedSession("first");
  const secondSession = savedSession("second");
  const project: ProjectSummary = {
    path: "/tmp/extension-ui-test",
    name: "extension-ui-test",
    workingDirectory: "/tmp/extension-ui-test",
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

  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = "";
  tau.state.activeSessionId = "";
  tau.state.activeSessionPath = "";
  tau.state.activeControllerKey = "";
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  await tau.initialize();
  tau.state.workspace = workspace;

  await tau.selectSession(project, firstSession);
  const firstController = tau.state.controllers.find(
    (controller) => controller.sessionId === firstSession.id,
  );
  if (!firstController) throw new Error("Expected the first controller");
  firstController.starting = false;
  firstController.ready = true;
  firstController.streaming = true;
  firstController.working = true;

  await tau.selectSession(project, secondSession);
  const secondController = tau.state.controllers.find(
    (controller) => controller.sessionId === secondSession.id,
  );
  if (!secondController) throw new Error("Expected the second controller");
  secondController.starting = false;
  secondController.ready = true;
  vi.mocked(invoke).mockClear();

  return {
    tau,
    project,
    firstSession,
    secondSession,
    firstController,
    secondController,
  };
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
