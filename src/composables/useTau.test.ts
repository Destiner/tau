import { invoke } from '@tauri-apps/api/core';
import { describe, expect, it, vi } from 'vitest';

import type { PiBridgeEvent } from '../lib/pi/bridge';

import type {
  ProjectSummary,
  SessionController,
  SessionSummary,
  WorkspaceSnapshot,
} from './state';
import useTau from './useTau';

type Tau = ReturnType<typeof useTau>;

const mocks = vi.hoisted(() => ({
  workspace: null as WorkspaceSnapshot | null,
  modelScope: [] as string[],
  generation: 0,
  listener: undefined as
    ((event: { payload: PiBridgeEvent }) => void) | undefined,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => {
    if (command.startsWith('start_pi')) {
      mocks.generation += 1;
      return mocks.generation;
    }
    if (command.endsWith('model_scope')) {
      return mocks.modelScope;
    }
    return mocks.workspace;
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
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

describe('session drafts and selection', () => {
  it('switches away from a working session while preserving its state', async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-draft-test',
      name: 'tau-draft-test',
      workingDirectory: '/tmp/tau-draft-test',
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
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

    state.activeProjectPath = '';
    state.activeSessionId = '';
    state.activeSessionPath = '';
    state.activeControllerKey = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await newSession(project);
    const first = projectSessions(project)[0];
    expect(first?.title).toBe('New session');

    draft.value = '  Keep this draft\nwith its session  ';
    expect(first?.title).toBe('Keep this draft with its session');
    expect(first && sessionIndicator(project, first)).toBe('draft');

    const firstController = state.controllers.find(
      (controller) => controller.sessionId === first?.id,
    );
    if (!first || !firstController) {
      throw new Error('Expected the first phantom session');
    }
    firstController.streaming = true;
    firstController.working = true;

    await newSession(project);
    expect(projectSessions(project)).toHaveLength(2);
    expect(sessionIndicator(project, first)).toBe('working');

    const empty = projectSessions(project)[0];
    expect(empty?.title).toBe('New session');
    await selectSession(project, first);

    expect(projectSessions(project)).toEqual([first]);
    expect(draft.value).toBe('  Keep this draft\nwith its session  ');
    expect(state.activeSessionId).toBe(first.id);
    expect(firstController.streaming).toBe(true);
  });

  it('starts a second runtime without stopping a working session', async () => {
    const firstSession = savedSession('first');
    const secondSession = savedSession('second');
    const project: ProjectSummary = {
      path: '/tmp/tau-concurrency-test',
      name: 'tau-concurrency-test',
      workingDirectory: '/tmp/tau-concurrency-test',
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const { state, initialize, selectSession, sessionIndicator } = useTau();
    await initialize();
    state.activeProjectPath = '';
    state.activeSessionId = '';
    state.activeSessionPath = '';
    state.activeControllerKey = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, firstSession);
    const firstController = state.controllers.find(
      (controller) => controller.sessionId === firstSession.id,
    );
    if (!firstController) throw new Error('Expected the first controller');
    firstController.starting = false;
    firstController.ready = true;
    firstController.streaming = true;
    firstController.working = true;

    await selectSession(project, secondSession);

    const startCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === 'start_pi');
    expect(startCalls).toHaveLength(2);
    expect(startCalls[0]?.[1]).not.toEqual(startCalls[1]?.[1]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === 'stop_pi' &&
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
      throw new Error('Expected the second controller and event listener');
    }
    emitTextDelta(firstController, 'background');
    emitTextDelta(secondController, 'foreground');
    await vi.waitFor(() => {
      expect(
        firstController.messages[firstController.messages.length - 1]?.text,
      ).toBe('background');
      expect(
        secondController.messages[secondController.messages.length - 1]?.text,
      ).toBe('foreground');
    });
    expect(firstController.unread).toBe(true);
    expect(secondController.unread).toBe(false);
    expect(sessionIndicator(project, secondSession)).toBe('');
  });

  it('keeps a manually marked session unread until it is read again', async () => {
    const firstSession = savedSession('first');
    const secondSession = savedSession('second');
    const unopenedSession = savedSession('unopened');
    const project: ProjectSummary = {
      path: '/tmp/tau-unread-test',
      name: 'tau-unread-test',
      workingDirectory: '/tmp/tau-unread-test',
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession, unopenedSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;

    const {
      state,
      selectSession,
      sessionIndicator,
      isSessionUnread,
      markSessionUnread,
      markSessionRead,
    } = useTau();
    state.activeProjectPath = '';
    state.activeSessionId = '';
    state.activeSessionPath = '';
    state.activeControllerKey = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, firstSession);
    expect(isSessionUnread(project, firstSession)).toBe(false);

    // The mark has to survive on the session being read, or it would only
    // appear once the sidebar selection moved elsewhere.
    markSessionUnread(project, firstSession);
    expect(isSessionUnread(project, firstSession)).toBe(true);
    expect(sessionIndicator(project, firstSession)).toBe('new');

    await selectSession(project, secondSession);
    expect(sessionIndicator(project, firstSession)).toBe('new');

    await selectSession(project, firstSession);
    expect(isSessionUnread(project, firstSession)).toBe(false);
    expect(sessionIndicator(project, firstSession)).toBe('');

    // A session Tau never opened still has to carry the mark.
    expect(sessionIndicator(project, unopenedSession)).toBe('');
    markSessionUnread(project, unopenedSession);
    expect(sessionIndicator(project, unopenedSession)).toBe('new');
    markSessionRead(project, unopenedSession);
    expect(sessionIndicator(project, unopenedSession)).toBe('');
  });

  it('keeps a submitted new session visible until its file is listed', async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-materialization-test',
      name: 'tau-materialization-test',
      workingDirectory: '/tmp/tau-materialization-test',
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
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
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await newSession(project);
    const controller = state.controllers[0];
    if (!controller) throw new Error('Expected a pending controller');

    const modelsRequest = sentRequests(controller, 'get_available_models')[0];
    emitRpc(controller, {
      id: modelsRequest?.id,
      type: 'response',
      command: 'get_available_models',
      success: true,
      data: {
        models: [
          {
            provider: 'provider',
            id: 'alpha',
            name: 'Alpha',
            reasoning: true,
          },
        ],
      },
    });
    const commandsRequest = sentRequests(controller, 'get_commands')[0];
    emitRpc(controller, {
      id: commandsRequest?.id,
      type: 'response',
      command: 'get_commands',
      success: true,
      data: {
        commands: [
          {
            name: 'session-name',
            description: 'Name this session',
            source: 'extension',
          },
          { name: 'skill:review', source: 'skill' },
          { name: 'ignored', source: 'unknown' },
        ],
      },
    });
    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: 'provisional',
        sessionFile: '/tmp/provisional.jsonl',
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.startMessagesRequestId).not.toBe('');
      expect(
        sentRequests(controller, 'get_available_thinking_levels'),
      ).toHaveLength(1);
    });
    const effortsRequest = sentRequests(
      controller,
      'get_available_thinking_levels',
    )[0];
    emitRpc(controller, {
      id: effortsRequest?.id,
      type: 'response',
      command: 'get_available_thinking_levels',
      success: true,
      data: { levels: ['off', 'high'] },
    });
    emitRpc(controller, {
      id: controller.startMessagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(controller.starting).toBe(false);
    });
    expect(controller.sessionId).toMatch(/^phantom-/);
    expect(models.value).toHaveLength(1);
    expect(efforts.value).toEqual(['off', 'high']);
    expect(commands.value).toEqual([
      {
        name: 'session-name',
        description: 'Name this session',
        source: 'extension',
      },
      { name: 'skill:review', source: 'skill' },
    ]);
    expect(currentModelLabel.value).toBe('Alpha');
    expect(currentEffortLabel.value).toBe('High');
    expect(settingsDisabled.value).toBe(false);

    draft.value = 'Start background work';
    await sendMessage();
    emitRpc(controller, {
      id: controller.pendingPrompt?.stateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: 'materialized',
        sessionFile: '/tmp/materialized.jsonl',
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.sessionId).toBe('materialized');
      expect(controller.pendingPrompt?.messagesRequestId).not.toBe('');
    });
    emitRpc(controller, {
      id: controller.pendingPrompt?.messagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(controller.pendingPrompt).toBeUndefined();
    });

    await newSession(project);

    expect(
      projectSessions(project).some((session) => session.id === 'materialized'),
    ).toBe(true);
  });

  it('keeps a fresh session at the top and applies inherited settings', async () => {
    const saved = savedSession('saved', 10_000);
    const project: ProjectSummary = {
      path: '/tmp/tau-new-session-settings-test',
      name: 'tau-new-session-settings-test',
      workingDirectory: '/tmp/tau-new-session-settings-test',
      collapsed: false,
      selected: true,
      sessions: [saved],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
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
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, saved);
    const savedController = state.controllers[0];
    if (!savedController) throw new Error('Expected the saved controller');
    savedController.starting = false;
    savedController.ready = true;
    savedController.models = [
      {
        provider: 'provider',
        id: 'alpha',
        name: 'Alpha',
        reasoning: true,
      },
      {
        provider: 'provider',
        id: 'beta',
        name: 'Beta',
        reasoning: true,
      },
    ];
    savedController.efforts = ['off', 'high', 'max'];
    savedController.commands = [{ name: 'session-name', source: 'extension' }];
    savedController.commandsLoaded = true;
    savedController.currentModelProvider = 'provider';
    savedController.currentModelId = 'alpha';
    savedController.currentModelName = 'Alpha';
    savedController.currentEffort = 'high';

    await newSession(project);

    expect(projectSessions(project)[0]?.title).toBe('New session');
    expect(models.value).toEqual(savedController.models);
    expect(currentModelLabel.value).toBe('Alpha');
    expect(currentEffortLabel.value).toBe('High');
    expect(settingsDisabled.value).toBe(false);

    await selectModel('provider/beta');
    await selectEffort('max');
    expect(currentModelId.value).toBe('beta');
    expect(currentModelLabel.value).toBe('Beta');
    expect(currentEffortLabel.value).toBe('Max');

    draft.value = 'Use these settings';
    await sendMessage();
    const controller = state.controllers.find(
      (candidate) => candidate !== savedController,
    );
    if (!controller) throw new Error('Expected the new controller');

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'off',
        sessionId: 'new-session',
        sessionFile: '/tmp/new-session.jsonl',
        sessionName: '',
        isStreaming: false,
      },
    });

    await vi.waitFor(() => {
      expect(sentRequests(controller, 'set_model')).toHaveLength(1);
    });
    const modelRequest = sentRequests(controller, 'set_model')[0];
    expect(modelRequest).toMatchObject({
      provider: 'provider',
      modelId: 'beta',
    });
    expect(sentRequests(controller, 'prompt')).toHaveLength(0);

    emitRpc(controller, {
      id: modelRequest?.id,
      type: 'response',
      command: 'set_model',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'set_thinking_level')).toHaveLength(1);
    });
    const effortRequest = sentRequests(controller, 'set_thinking_level')[0];
    expect(effortRequest).toMatchObject({ level: 'max' });
    expect(sentRequests(controller, 'prompt')).toHaveLength(0);

    emitRpc(controller, {
      id: effortRequest?.id,
      type: 'response',
      command: 'set_thinking_level',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_messages')).toHaveLength(1);
    });
    const messagesRequest = sentRequests(controller, 'get_messages')[0];
    emitRpc(controller, {
      id: messagesRequest?.id,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'prompt')).toHaveLength(1);
    });
  });

  it('archives a session without stopping its running controller', async () => {
    const firstSession = savedSession('first', 2_000);
    const secondSession = savedSession('second', 1_000);
    const project: ProjectSummary = {
      path: '/tmp/tau-archive-test',
      name: 'tau-archive-test',
      workingDirectory: '/tmp/tau-archive-test',
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const { archiveSession, projectSessions, selectSession, state } = useTau();
    state.activeProjectPath = '';
    state.activeSessionId = '';
    state.activeSessionPath = '';
    state.activeControllerKey = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;

    await selectSession(project, firstSession);
    const firstController = state.controllers.find(
      (controller) => controller.sessionId === firstSession.id,
    );
    if (!firstController) throw new Error('Expected the first controller');
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
            command === 'archive_session' &&
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
            command === 'stop_pi' &&
            (args as { runtimeId?: string })?.runtimeId ===
              firstController.runtimeId,
        ),
    ).toBe(false);
  });

  it('reorders sessions only when the user submits a message', async () => {
    const firstSession = savedSession('first', 1_000);
    const secondSession = savedSession('second', 2_000);
    const project: ProjectSummary = {
      path: '/tmp/tau-order-test',
      name: 'tau-order-test',
      workingDirectory: '/tmp/tau-order-test',
      collapsed: false,
      selected: true,
      sessions: [firstSession, secondSession],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
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
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    expect(projectSessions(project)[0]?.id).toBe('second');
    await selectSession(project, firstSession);
    expect(canDraft.value).toBe(true);
    expect(canCompose.value).toBe(false);
    draft.value = 'Move this session to the top';
    const firstController = state.controllers[0];
    if (!firstController) throw new Error('Expected the first controller');
    firstController.starting = false;
    firstController.ready = true;
    await sendMessage();
    expect(projectSessions(project)[0]?.id).toBe('first');

    await selectSession(project, secondSession);
    const secondController = state.controllers.find(
      (controller) => controller.sessionId === secondSession.id,
    );
    if (!secondController) throw new Error('Expected the second controller');
    emitTextDelta(secondController, 'Assistant-only update');
    await vi.waitFor(() => {
      expect(secondController.messages).toHaveLength(1);
    });

    expect(projectSessions(project)[0]?.id).toBe('first');
  });

  it('reports a saved session as loading until its transcript hydrates', async () => {
    const session = savedSession('hydrating');
    const project: ProjectSummary = {
      path: '/tmp/tau-loading-test',
      name: 'tau-loading-test',
      workingDirectory: '/tmp/tau-loading-test',
      collapsed: false,
      selected: true,
      sessions: [session],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    const { state, initialize, newSession, selectSession, sessionLoading } =
      useTau();
    await initialize();
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await selectSession(project, session);
    const controller = state.controllers[0];
    if (!controller) throw new Error('Expected a controller');
    expect(sessionLoading.value).toBe(true);

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: session.id,
        sessionFile: session.path,
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.startMessagesRequestId).not.toBe('');
    });
    expect(sessionLoading.value).toBe(true);

    emitRpc(controller, {
      id: controller.startMessagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'user', content: 'Restored from the session file' }],
      },
    });
    await vi.waitFor(() => {
      expect(controller.starting).toBe(false);
    });
    expect(sessionLoading.value).toBe(false);

    await newSession(project);
    expect(sessionLoading.value).toBe(false);
  });
});

describe('session replacement hardening', () => {
  it('rebinds a background workflow controller without stealing the selected session', async () => {
    const {
      firstController,
      firstSession,
      secondController,
      secondSession,
      project,
      tau,
    } = await setupExtensionControllers();
    firstController.messages.push({
      id: 'old-phase-message',
      kind: 'assistant',
      text: 'Old phase',
    });

    emitRpc(firstController, { type: 'agent_start' });
    await vi.waitFor(() => {
      expect(sentRequests(firstController, 'get_state')).toHaveLength(1);
    });
    const runStateRequest = sentRequests(firstController, 'get_state')[0];
    const replacementSession = {
      ...savedSession('plan-phase'),
      title: 'Plan phase',
    };
    const workspace = mocks.workspace;
    if (!workspace) throw new Error('Expected the extension workspace');
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
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'beta', name: 'Beta' },
        thinkingLevel: 'xhigh',
        sessionId: replacementSession.id,
        sessionFile: replacementSession.path,
        sessionName: replacementSession.title,
        isStreaming: true,
      },
    });

    await vi.waitFor(() => {
      expect(firstController.sessionId).toBe(replacementSession.id);
      expect(
        sentRequests(firstController, 'get_available_models'),
      ).toHaveLength(1);
      expect(sentRequests(firstController, 'get_commands')).toHaveLength(1);
      expect(
        sentRequests(firstController, 'get_available_thinking_levels'),
      ).toHaveLength(1);
      expect(sentRequests(firstController, 'get_messages')).toHaveLength(1);
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
            command === 'register_session' &&
            (args as { sessionId?: string })?.sessionId ===
              replacementSession.id,
        ),
    ).toBe(true);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'set_active_session'),
    ).toBe(false);

    const commandsRequest = sentRequests(firstController, 'get_commands')[0];
    emitRpc(firstController, {
      id: commandsRequest?.id,
      type: 'response',
      command: 'get_commands',
      success: true,
      data: {
        commands: [
          {
            name: 'workflow-next',
            description: 'Continue the workflow',
            source: 'extension',
          },
        ],
      },
    });
    const messagesRequest = sentRequests(firstController, 'get_messages')[0];
    emitRpc(firstController, {
      id: messagesRequest?.id,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'user', content: 'Review the plan' }],
      },
    });
    await vi.waitFor(() => {
      expect(firstController.commands).toEqual([
        {
          name: 'workflow-next',
          description: 'Continue the workflow',
          source: 'extension',
        },
      ]);
      expect(firstController.messages).toEqual([
        { id: 'user-0', kind: 'user', text: 'Review the plan' },
      ]);
    });

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'replacement-approval',
      method: 'confirm',
      title: 'Approve the plan?',
      message: 'The replacement session is waiting.',
    });
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    const replacementProject = tau.state.workspace?.projects[0];
    const replacementView = replacementProject?.sessions.find(
      (session) => session.id === replacementSession.id,
    );
    if (!replacementProject || !replacementView) {
      throw new Error('Expected the registered replacement session');
    }

    await tau.selectSession(replacementProject, replacementView);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      controllerKey: firstController.key,
      runtimeId: firstController.runtimeId,
      sessionName: 'Plan phase',
    });
    await tau.submitExtensionDialog(true);
    expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
      {
        type: 'extension_ui_response',
        id: 'replacement-approval',
        confirmed: true,
      },
    ]);
    tau.dispose();
  });

  it('registers a session an extension command spawns without echoing the command', async () => {
    const { secondController, secondSession, project, tau } =
      await setupExtensionControllers();
    secondController.commands = [{ name: 'mock', source: 'extension' }];
    secondController.commandsLoaded = true;

    tau.draft.value = '/mock 42';
    await tau.sendMessage();

    expect(secondController.messages).toEqual([]);
    expect(secondController.lastUserMessageAt).toBe(0);
    const promptRequest = sentRequests(secondController, 'prompt')[0];
    expect(promptRequest?.message).toBe('/mock 42');
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'register_session'),
    ).toBe(false);

    const phaseSession = { ...savedSession('plan-phase'), title: '42 • plan' };
    mocks.workspace = {
      ...(mocks.workspace as WorkspaceSnapshot),
      projects: [{ ...project, sessions: [secondSession, phaseSession] }],
    };
    emitRpc(secondController, {
      id: promptRequest?.id,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(secondController, 'get_state')).toHaveLength(1);
    });

    emitRpc(secondController, {
      id: sentRequests(secondController, 'get_state')[0]?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: phaseSession.id,
        sessionFile: phaseSession.path,
        sessionName: phaseSession.title,
        isStreaming: false,
      },
    });

    await vi.waitFor(() => {
      expect(secondController.sessionId).toBe(phaseSession.id);
      expect(tau.state.activeSessionId).toBe(phaseSession.id);
    });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === 'register_session' &&
            (args as { sessionId?: string })?.sessionId === phaseSession.id,
        ),
    ).toBe(true);
    expect(secondController.messages).toEqual([]);
    tau.dispose();
  });

  it('registers only the spawned session when a command starts a new session', async () => {
    const { project, tau } = await setupExtensionControllers();
    await tau.newSession(project);
    const controller = tau.state.controllers.find(
      (candidate) => candidate.phantom,
    );
    if (!controller) throw new Error('Expected a phantom controller');
    controller.starting = false;
    controller.ready = true;
    controller.commands = [{ name: 'mock', source: 'extension' }];
    controller.commandsLoaded = true;
    controller.currentEffort = 'high';
    vi.mocked(invoke).mockClear();

    tau.draft.value = '/mock 42';
    await tau.sendMessage();

    expect(controller.messages).toEqual([]);
    emitRpc(controller, {
      id: controller.pendingPrompt?.stateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: 'unwritten',
        sessionFile: '/tmp/unwritten.jsonl',
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.pendingPrompt?.messagesRequestId).not.toBe('');
    });

    // Pi never writes a session that holds no assistant message, so the
    // session the command replaces must not reach the workspace.
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'register_session'),
    ).toBe(false);

    emitRpc(controller, {
      id: controller.pendingPrompt?.messagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'prompt')).toHaveLength(1);
    });
    expect(controller.messages).toEqual([]);

    const phaseSession = { ...savedSession('plan-phase'), title: '42 • plan' };
    mocks.workspace = {
      ...(mocks.workspace as WorkspaceSnapshot),
      projects: [{ ...project, sessions: [...project.sessions, phaseSession] }],
    };
    emitRpc(controller, {
      id: sentRequests(controller, 'prompt')[0]?.id,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_state')).toHaveLength(2);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state')[1]?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: phaseSession.id,
        sessionFile: phaseSession.path,
        sessionName: phaseSession.title,
        isStreaming: false,
      },
    });

    await vi.waitFor(() => {
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(([command]) => command === 'register_session'),
      ).toHaveLength(1);
    });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.find(([command]) => command === 'register_session')?.[1],
    ).toMatchObject({ sessionId: phaseSession.id });
    expect(controller.sessionId).toBe(phaseSession.id);
    expect(controller.phantom).toBe(false);

    // The row for the replaced session must not outlive the command that
    // replaced it, or it lingers as a session nobody can open.
    const registered = tau.state.workspace?.projects[0];
    if (!registered) throw new Error('Expected the registered project');
    const titles = tau.projectSessions(registered).map((item) => item.title);
    expect(titles).toContain('42 • plan');
    expect(titles).not.toContain('/mock 42');
    tau.dispose();
  });

  it('detects a phase session opened after the run settles', async () => {
    vi.useFakeTimers();
    try {
      const { firstController, firstSession, secondController, project, tau } =
        await setupExtensionControllers();
      firstController.streaming = true;
      firstController.working = true;

      emitRpc(firstController, { type: 'agent_settled' });
      await vi.waitFor(() => {
        expect(sentRequests(firstController, 'get_state')).toHaveLength(1);
      });
      emitRpc(firstController, {
        id: sentRequests(firstController, 'get_state')[0]?.id,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
          thinkingLevel: 'high',
          sessionId: firstSession.id,
          sessionFile: firstSession.path,
          sessionName: firstSession.title,
          isStreaming: false,
        },
      });
      await vi.waitFor(() => {
        expect(sentRequests(firstController, 'get_messages')).toHaveLength(1);
      });
      emitRpc(firstController, {
        id: sentRequests(firstController, 'get_messages')[0]?.id,
        type: 'response',
        command: 'get_messages',
        success: true,
        data: { messages: [] },
      });

      // The runtime stays alive while the workflow may still open its next
      // session, even though nothing is selecting this hidden controller.
      await vi.advanceTimersByTimeAsync(140);
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === 'stop_pi'),
      ).toBe(false);

      const phaseSession = {
        ...savedSession('execute-phase'),
        title: '42 • execute',
      };
      mocks.workspace = {
        ...(mocks.workspace as WorkspaceSnapshot),
        projects: [{ ...project, sessions: [firstSession, phaseSession] }],
      };
      await vi.advanceTimersByTimeAsync(60);
      const probeRequest = sentRequests(firstController, 'get_state')[1];
      expect(probeRequest).toBeDefined();

      emitRpc(firstController, {
        id: probeRequest?.id,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
          thinkingLevel: 'high',
          sessionId: phaseSession.id,
          sessionFile: phaseSession.path,
          sessionName: phaseSession.title,
          isStreaming: false,
        },
      });
      await vi.waitFor(() => {
        expect(firstController.sessionId).toBe(phaseSession.id);
      });
      expect(
        vi
          .mocked(invoke)
          .mock.calls.some(
            ([command, args]) =>
              command === 'register_session' &&
              (args as { sessionId?: string })?.sessionId === phaseSession.id,
          ),
      ).toBe(true);
      expect(tau.state.activeControllerKey).toBe(secondController.key);
      expect(
        vi
          .mocked(invoke)
          .mock.calls.some(([command]) => command === 'set_active_session'),
      ).toBe(false);

      // A detected replacement ends the watch instead of polling it out.
      await vi.advanceTimersByTimeAsync(4_000);
      expect(sentRequests(firstController, 'get_state')).toHaveLength(2);
      tau.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('uses an identity-only state request for an unchanged agent run', async () => {
    const { secondController, secondSession, tau } =
      await setupExtensionControllers();

    emitRpc(secondController, { type: 'agent_start' });
    await vi.waitFor(() => {
      expect(sentRequests(secondController, 'get_state')).toHaveLength(1);
    });
    const runStateRequest = sentRequests(secondController, 'get_state')[0];
    emitRpc(secondController, {
      id: runStateRequest?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: secondSession.id,
        sessionFile: secondSession.path,
        sessionName: secondSession.title,
        isStreaming: true,
      },
    });
    await vi.waitFor(() => {
      expect(secondController.runStateRequestId).toBe('');
    });

    expect(sentRequests(secondController, 'get_available_models')).toEqual([]);
    expect(sentRequests(secondController, 'get_commands')).toEqual([]);
    expect(
      sentRequests(secondController, 'get_available_thinking_levels'),
    ).toEqual([]);
    expect(sentRequests(secondController, 'get_messages')).toEqual([]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'register_session'),
    ).toBe(false);
    tau.dispose();
  });

  it('retires a session Pi never saved instead of opening a copy of it', async () => {
    const { controller, ghost, other, project, tau } =
      await setupUnsavedSession();

    // Pi answers --session for a file it cannot find by opening a fresh
    // session under the path it was handed.
    mocks.workspace = {
      ...(mocks.workspace as WorkspaceSnapshot),
      projects: [
        { ...project, sessions: [{ ...ghost, archived: true }, other] },
      ],
    };
    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: 'minted-by-pi',
        sessionFile: ghost.path,
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.phantom).toBe(true);
    });

    // Registering the minted id would file a second session at the same path,
    // and the row would go on minting one more on every visit.
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'register_session'),
    ).toBe(false);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.find(([command]) => command === 'archive_session')?.[1],
    ).toMatchObject({ sessionId: ghost.id });

    const registered = tau.state.workspace?.projects[0];
    if (!registered) throw new Error('Expected the registered project');
    const rows = tau.projectSessions(registered);
    expect(rows.map((row) => row.title)).toEqual(['New session', other.title]);
    expect(tau.state.activeSessionId).toBe(rows[0]?.id);
    expect(controller.status).toBe(
      'That session was never saved by Pi, so this is a new one.',
    );

    emitRpc(controller, {
      id: sentRequests(controller, 'get_messages')[0]?.id,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });

    // The replacement is unsent, so it leaves no trace once it is left.
    await tau.selectSession(registered, other);
    expect(tau.state.ephemeralSessions).toEqual([]);
    tau.dispose();
  });

  it('registers a session an extension opens while Tau is connecting', async () => {
    const { controller, ghost, tau } = await setupUnsavedSession();

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: 'opened-phase',
        sessionFile: '/tmp/opened-phase.jsonl',
        sessionName: 'Phase',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.sessionId).toBe('opened-phase');
    });

    // A replacement brings its own path, so it is a session Pi holds rather
    // than one it could not find.
    expect(controller.phantom).toBe(false);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.find(([command]) => command === 'register_session')?.[1],
    ).toMatchObject({ sessionId: 'opened-phase' });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'archive_session'),
    ).toBe(false);
    expect(ghost.id).not.toBe(controller.sessionId);
    tau.dispose();
  });
});

describe('extension UI protocol', () => {
  it('keeps dialogs in their originating session while users switch freely', async () => {
    const {
      firstController,
      firstSession,
      secondController,
      secondSession,
      project,
      tau,
    } = await setupExtensionControllers();

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'reviewer-1',
      method: 'select',
      title: 'Choose a reviewer',
      options: ['[ ] Claude', 'Done'],
    });
    emitRpc(secondController, {
      type: 'extension_ui_request',
      id: 'merge-1',
      method: 'confirm',
      title: 'Merge the pull request?',
      message: 'This requires manual approval.',
    });

    expect(tau.state.extensionDialogs).toHaveLength(2);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      requestId: 'merge-1',
      method: 'confirm',
      sessionName: 'second',
    });

    await tau.selectSession(project, firstSession);
    expect(tau.activeExtensionDialog.value).toMatchObject({
      requestId: 'reviewer-1',
      method: 'select',
      projectName: 'extension-ui-test',
      sessionName: 'first',
    });
    await tau.submitExtensionDialog('[ ] Claude');
    expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
      {
        type: 'extension_ui_response',
        id: 'reviewer-1',
        value: '[ ] Claude',
      },
    ]);

    await tau.selectSession(project, secondSession);
    expect(tau.activeExtensionDialog.value?.requestId).toBe('merge-1');
    await tau.submitExtensionDialog(false);
    expect(sentRequests(secondController, 'extension_ui_response')).toEqual([
      {
        type: 'extension_ui_response',
        id: 'merge-1',
        confirmed: false,
      },
    ]);
    expect(tau.activeExtensionDialog.value).toBeUndefined();

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'reviewer-2',
      method: 'select',
      title: 'Choose a reviewer',
      options: ['[x] Claude', 'Done'],
    });
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    await tau.selectSession(project, firstSession);
    await tau.submitExtensionDialog('Done');

    expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
      {
        type: 'extension_ui_response',
        id: 'reviewer-1',
        value: '[ ] Claude',
      },
      {
        type: 'extension_ui_response',
        id: 'reviewer-2',
        value: 'Done',
      },
    ]);
    tau.dispose();
  });

  it('returns input values and explicit cancellations with the RPC shapes', async () => {
    const { firstController, firstSession, secondSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'issue-id',
      method: 'input',
      title: 'Linear issue ID',
      placeholder: 'ENG-123',
    });
    if (!tau.activeExtensionDialog.value) {
      throw new Error('Expected the input prompt');
    }
    tau.activeExtensionDialog.value.draft = 'ENG-42';
    await tau.selectSession(project, secondSession);
    expect(tau.activeExtensionDialog.value).toBeUndefined();
    await tau.selectSession(project, firstSession);
    expect(tau.activeExtensionDialog.value?.draft).toBe('ENG-42');
    await tau.submitExtensionDialog(
      tau.activeExtensionDialog.value?.draft ?? '',
    );

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'task-notes',
      method: 'editor',
      title: 'Task notes',
      prefill: 'Keep the API stable.',
    });
    await tau.cancelExtensionDialog();

    expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
      {
        type: 'extension_ui_response',
        id: 'issue-id',
        value: 'ENG-42',
      },
      {
        type: 'extension_ui_response',
        id: 'task-notes',
        cancelled: true,
      },
    ]);
    tau.dispose();
  });

  it('keeps extension drafts and notices in their owning session', async () => {
    const { firstController, firstSession, secondController, project, tau } =
      await setupExtensionControllers();
    firstController.status = 'Tau connection warning';

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'editor-text-1',
      method: 'set_editor_text',
      text: '/implement',
    });
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'status-1',
      method: 'setStatus',
      statusKey: 'mcp',
      statusText: '\u001b[38;2;138;190;183mMCP ready\u001b[39m',
    });
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'notify-info',
      method: 'notify',
      message: 'MCP servers refreshed',
    });
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'notify-warning',
      method: 'notify',
      message: 'Approval is ready',
      notifyType: 'warning',
    });
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'notify-error',
      method: 'notify',
      message: 'MCP server failed',
      notifyType: 'error',
    });

    expect(firstController.draft).toBe('/implement');
    expect(firstController.status).toBe('Tau connection warning');
    expect(firstController.unread).toBe(true);
    expect(secondController.messages).toEqual([]);
    expect(firstController.messages).toEqual([
      expect.objectContaining({
        kind: 'notice',
        text: 'MCP servers refreshed',
        noticeType: 'info',
        basePath: '/tmp/extension-ui-test',
      }),
      expect.objectContaining({
        kind: 'notice',
        text: 'Approval is ready',
        noticeType: 'warning',
      }),
      expect.objectContaining({
        kind: 'notice',
        text: 'MCP server failed',
        noticeType: 'error',
      }),
    ]);

    emitRpc(firstController, {
      type: 'response',
      id: 'hydrate-after-command',
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'user', content: 'Previous prompt' }] },
    });
    await vi.waitFor(() => {
      expect(firstController.messages.map((message) => message.kind)).toEqual([
        'user',
        'notice',
        'notice',
        'notice',
      ]);
    });

    await tau.selectSession(project, firstSession);

    expect(tau.draft.value).toBe('/implement');
    expect(tau.status.value).toBe('');
    expect(tau.messages.value).toBe(firstController.messages);
    tau.dispose();
  });

  it('marks a background session waiting on a prompt as unread', async () => {
    const { firstController, firstSession, secondSession, project, tau } =
      await setupExtensionControllers();

    expect(tau.sessionIndicator(project, firstSession)).toBe('working');

    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'reviewer-1',
      method: 'select',
      title: 'Choose a reviewer',
      options: ['Claude', 'Done'],
    });

    expect(firstController.unread).toBe(true);
    expect(tau.sessionIndicator(project, firstSession)).toBe('new');
    expect(
      tau.indicatorLabel(tau.sessionIndicator(project, firstSession)),
    ).toBe('Unread');
    expect(tau.sessionIndicator(project, secondSession)).toBe('');

    project.collapsed = true;
    expect(tau.projectIndicator(project)).toBe('new');
    project.collapsed = false;
    expect(tau.projectIndicator(project)).toBe('');

    await tau.selectSession(project, firstSession);
    expect(firstController.unread).toBe(false);
    await tau.submitExtensionDialog('Claude');
    expect(tau.sessionIndicator(project, firstSession)).toBe('working');
    tau.dispose();
  });

  it('discards dialogs after their timeout, generation change, or process exit', async () => {
    const { firstController, firstSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);
    vi.useFakeTimers();

    try {
      emitRpc(firstController, {
        type: 'extension_ui_request',
        id: 'timed-1',
        method: 'input',
        title: 'Optional issue ID',
        timeout: 25,
      });
      expect(tau.activeExtensionDialog.value?.requestId).toBe('timed-1');

      vi.advanceTimersByTime(25);
      expect(tau.activeExtensionDialog.value).toBeUndefined();
      expect(sentRequests(firstController, 'extension_ui_response')).toEqual(
        [],
      );

      emitRpc(firstController, {
        type: 'extension_ui_request',
        id: 'old-generation',
        method: 'editor',
        title: 'Task notes',
        prefill: 'Initial notes',
      });
      const nextGeneration = firstController.generation + 1;
      emitBridge(firstController.runtimeId, nextGeneration, 'started');
      expect(tau.activeExtensionDialog.value).toBeUndefined();
      expect(firstController.generation).toBe(nextGeneration);

      emitRpc(firstController, {
        type: 'extension_ui_request',
        id: 'exiting',
        method: 'confirm',
        title: 'Continue?',
        message: 'The process is about to exit.',
      });
      expect(tau.activeExtensionDialog.value?.requestId).toBe('exiting');
      emitBridge(firstController.runtimeId, nextGeneration, 'exited', 0);
      expect(tau.activeExtensionDialog.value).toBeUndefined();
    } finally {
      vi.useRealTimers();
      tau.dispose();
    }
  });
});

describe('project ordering', () => {
  it('persists the complete reordered project path list', async () => {
    const projects = ['alpha', 'beta', 'gamma'].map(
      (name, index): ProjectSummary => ({
        path: `/tmp/${name}`,
        name,
        workingDirectory: `/tmp/${name}`,
        collapsed: false,
        selected: index === 0,
        sessions: [],
      }),
    );
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: projects[0]?.path ?? '',
      piPath: '/usr/local/bin/pi',
      projects,
    };
    const reordered = {
      ...workspace,
      projects: [projects[1], projects[2], projects[0]].filter(
        (project): project is ProjectSummary => Boolean(project),
      ),
    };
    mocks.workspace = workspace;
    const tau = useTau();
    tau.state.workspace = workspace;
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockResolvedValueOnce(reordered);

    await tau.reorderProjects(0, 2);

    expect(
      tau.state.workspace?.projects.map((project) => project.name),
    ).toEqual(['beta', 'gamma', 'alpha']);
    expect(invoke).toHaveBeenCalledWith(
      'reorder_projects',
      expect.objectContaining({
        projectPaths: ['/tmp/beta', '/tmp/gamma', '/tmp/alpha'],
      }),
    );
  });
});

describe('model scope', () => {
  it('offers only the models Pi has scoped for the project', async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-model-scope-test',
      name: 'tau-model-scope-test',
      workingDirectory: '/tmp/tau-model-scope-test',
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.modelScope = ['provider/beta'];

    const { state, initialize, models, newSession } = useTau();
    await initialize();
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await newSession(project);
    const controller = state.controllers[0];
    if (!controller) throw new Error('Expected a pending controller');

    const modelsRequest = sentRequests(controller, 'get_available_models')[0];
    emitRpc(controller, {
      id: modelsRequest?.id,
      type: 'response',
      command: 'get_available_models',
      success: true,
      data: {
        models: [
          { provider: 'provider', id: 'alpha', name: 'Alpha', reasoning: true },
          { provider: 'provider', id: 'beta', name: 'Beta', reasoning: true },
        ],
      },
    });

    await vi.waitFor(() =>
      expect(models.value.map((model) => model.id)).toEqual(['beta']),
    );

    mocks.modelScope = [];
  });

  it("keeps the scope on a session that inherits a warm runtime's models", async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-model-scope-inherit-test',
      name: 'tau-model-scope-inherit-test',
      workingDirectory: '/tmp/tau-model-scope-inherit-test',
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    mocks.modelScope = ['provider/beta'];

    const { state, initialize, models, newSession } = useTau();
    await initialize();
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await newSession(project);
    const first = state.controllers[0];
    if (!first) throw new Error('Expected a pending controller');

    const modelsRequest = sentRequests(first, 'get_available_models')[0];
    emitRpc(first, {
      id: modelsRequest?.id,
      type: 'response',
      command: 'get_available_models',
      success: true,
      data: {
        models: [
          { provider: 'provider', id: 'alpha', name: 'Alpha', reasoning: true },
          { provider: 'provider', id: 'beta', name: 'Beta', reasoning: true },
        ],
      },
    });
    await vi.waitFor(() =>
      expect(models.value.map((model) => model.id)).toEqual(['beta']),
    );

    // A session that has been used carries everything the next one inherits,
    // which is what lets the next one open without starting a runtime.
    first.draft = 'Keep this session';
    first.efforts = ['off'];
    first.commandsLoaded = true;
    first.currentModelProvider = 'provider';
    first.currentModelId = 'beta';
    first.currentModelName = 'Beta';

    await newSession(project);
    expect(state.controllers).toHaveLength(2);
    expect(models.value.map((model) => model.id)).toEqual(['beta']);

    mocks.modelScope = [];
  });
});

describe('transcript continuity', () => {
  it('keeps streamed row ids when a settled turn rehydrates', async () => {
    const session = savedSession('continuity');
    const project: ProjectSummary = {
      path: '/tmp/tau-continuity-test',
      name: 'tau-continuity-test',
      workingDirectory: '/tmp/tau-continuity-test',
      collapsed: false,
      selected: true,
      sessions: [session],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    mocks.workspace = workspace;
    const { state, initialize, selectSession } = useTau();
    await initialize();
    state.activeControllerKey = '';
    state.activeSessionId = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);

    await selectSession(project, session);
    const controller = state.controllers[0];
    if (!controller) throw new Error('Expected a controller');
    const sessionState = (isStreaming: boolean): Record<string, unknown> => ({
      model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
      thinkingLevel: 'high',
      sessionId: session.id,
      sessionFile: session.path,
      sessionName: '',
      isStreaming,
    });

    emitRpc(controller, {
      id: controller.bootstrapStateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: sessionState(false),
    });
    await vi.waitFor(() => {
      expect(controller.startMessagesRequestId).not.toBe('');
    });
    emitRpc(controller, {
      id: controller.startMessagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'user', content: 'Inspect the project' }] },
    });
    await vi.waitFor(() => {
      expect(controller.messages).toHaveLength(1);
    });

    emitRpc(controller, { type: 'agent_start' });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_state')).toHaveLength(2);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state')[1]?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: sessionState(true),
    });
    emitTextDelta(controller, 'Partial');
    await vi.waitFor(() => {
      expect(controller.messages).toHaveLength(2);
    });
    const streamed = controller.messages.map((message) => message.id);

    emitRpc(controller, { type: 'agent_settled' });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_state')).toHaveLength(3);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state')[2]?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: sessionState(false),
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_messages')).toHaveLength(2);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_messages')[1]?.id,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'user', content: 'Inspect the project' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Partial reply, completed.' }],
          },
        ],
      },
    });

    // A settled turn rewrites rows the reader may be scrolled into, so the
    // virtualizer has to recognize them or it loses their measured heights.
    await vi.waitFor(() => {
      expect(controller.messages[1]?.text).toBe('Partial reply, completed.');
    });
    expect(controller.messages.map((message) => message.id)).toEqual(streamed);
  });
});

describe('interrupting a run', () => {
  it('reopens a session Pi never acknowledged stopping', async () => {
    vi.useFakeTimers();
    try {
      const { tau, controller, session } = await setupNamedSession();
      controller.streaming = true;
      controller.working = true;
      emitRpc(controller, {
        type: 'tool_execution_start',
        toolCallId: 'call-1',
        toolName: 'bash',
        args: { command: 'sleep 600' },
      });

      await tau.stop();
      expect(sentRequests(controller, 'abort')).toHaveLength(1);
      expect(tau.stopping.value).toBe(true);

      // Pi answers the abort only once the agent is idle, so a tool call that
      // outlives it must not hold the session in a stop that never lands.
      await vi.advanceTimersByTimeAsync(2_000);
      const probe = sentRequests(controller, 'get_state')[0];
      expect(probe).toBeDefined();
      emitRpc(controller, {
        id: probe?.id,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
          thinkingLevel: 'high',
          sessionId: session.id,
          sessionFile: session.path,
          sessionName: session.title,
          isStreaming: true,
        },
      });

      await vi.waitFor(() => {
        expect(tau.stopping.value).toBe(false);
      });
      expect(tau.streaming.value).toBe(true);
      expect(tau.status.value).toBe(
        'Pi is still running bash and stops once it returns.',
      );
      // Rehydrating mid-run would drop the deltas the run is still streaming.
      expect(sentRequests(controller, 'get_messages')).toEqual([]);

      // The stop the user asked for still arrives, and clears the notice.
      emitRpc(controller, { type: 'agent_settled' });
      await vi.waitFor(() => {
        expect(tau.streaming.value).toBe(false);
      });
      expect(tau.status.value).toBe('');
      tau.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('unlocks a session whose settle event never reached Tau', async () => {
    vi.useFakeTimers();
    try {
      const { tau, controller, session } = await setupNamedSession();
      controller.streaming = true;
      controller.working = true;

      await tau.stop();
      await vi.advanceTimersByTimeAsync(2_000);
      const probe = sentRequests(controller, 'get_state')[0];
      emitRpc(controller, {
        id: probe?.id,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
          thinkingLevel: 'high',
          sessionId: session.id,
          sessionFile: session.path,
          sessionName: session.title,
          isStreaming: false,
        },
      });

      await vi.waitFor(() => {
        expect(tau.streaming.value).toBe(false);
      });
      expect(tau.stopping.value).toBe(false);
      expect(tau.status.value).toBe('');
      await vi.waitFor(() => {
        expect(sentRequests(controller, 'get_messages')).toHaveLength(1);
      });
      tau.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('leaves an acknowledged stop alone', async () => {
    vi.useFakeTimers();
    try {
      const { tau, controller } = await setupNamedSession();
      controller.streaming = true;
      controller.working = true;

      await tau.stop();
      emitRpc(controller, { type: 'agent_settled' });
      await vi.waitFor(() => {
        expect(tau.stopping.value).toBe(false);
      });

      await vi.advanceTimersByTimeAsync(2_000);
      expect(
        sentRequests(controller, 'get_state').filter((request) =>
          String(request.id).includes('abort-probe'),
        ),
      ).toEqual([]);
      tau.dispose();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('turn failures', () => {
  const credits = `402: {"message":"This request requires more credits.","code":402}`;

  /** Settling from Pi's messages costs a round trip, and a retry can delay it. */
  it('shows a failed turn as soon as Pi reports it', async () => {
    const { tau, controller } = await setupNamedSession();

    emitRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [],
        stopReason: 'error',
        errorMessage: credits,
      },
    });

    await vi.waitFor(() => {
      expect(controller.messages).toHaveLength(1);
    });
    expect(controller.messages[0]).toMatchObject({
      kind: 'error',
      text: credits,
    });
    tau.dispose();
  });

  it('holds one failure through the settle that rewrites the turn', async () => {
    const { tau, controller, session } = await setupNamedSession();

    emitRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [],
        stopReason: 'error',
        errorMessage: credits,
      },
    });
    await vi.waitFor(() => {
      expect(controller.messages).toHaveLength(1);
    });

    await settleWith(controller, session, [
      { role: 'user', content: 'hello' },
      {
        role: 'assistant',
        content: [],
        stopReason: 'error',
        errorMessage: credits,
      },
    ]);

    expect(controller.messages.map((entry) => entry.kind)).toEqual([
      'user',
      'error',
    ]);
    tau.dispose();
  });

  it("keeps a failed compaction, which Pi's messages never carry", async () => {
    const { tau, controller, session } = await setupNamedSession();

    emitRpc(controller, {
      type: 'compaction_end',
      reason: 'threshold',
      aborted: false,
      willRetry: false,
      errorMessage: 'Auto-compaction failed: overloaded',
    });
    await vi.waitFor(() => {
      expect(controller.messages).toHaveLength(1);
    });

    await settleWith(controller, session, [{ role: 'user', content: 'hello' }]);

    expect(controller.messages.map((entry) => entry.kind)).toEqual([
      'user',
      'error',
    ]);
    expect(controller.messages[1]?.text).toBe(
      'Auto-compaction failed: overloaded',
    );
    tau.dispose();
  });

  it('says what it is retrying rather than only that it is', async () => {
    const { tau, controller } = await setupNamedSession();

    emitRpc(controller, {
      type: 'auto_retry_start',
      attempt: 1,
      maxAttempts: 3,
      delayMs: 1_000,
      errorMessage: `529 {"type":"error","error":{"message":"Overloaded"}}`,
    });

    await vi.waitFor(() => {
      expect(controller.status).toBe('Retrying (1/3): Overloaded');
    });
    tau.dispose();
  });
});

describe('session naming', () => {
  it('renames the selected session optimistically and stores the echoed name', async () => {
    const { tau, project, controller } = await setupNamedSession();
    expect(tau.canRenameSession.value).toBe(true);

    const rename = tau.renameSession('  Migration   plan  ');
    expect(tau.sessionTitle.value).toBe('Migration plan');
    expect(tau.projectSessions(project)[0]?.title).toBe('Migration plan');
    await rename;
    expect(sentRequests(controller, 'set_session_name')).toEqual([
      {
        id: expect.any(String),
        type: 'set_session_name',
        name: 'Migration plan',
      },
    ]);

    emitRpc(controller, {
      id: sentRequests(controller, 'set_session_name')[0]?.id,
      type: 'response',
      command: 'set_session_name',
      success: true,
    });
    emitRpc(controller, {
      type: 'session_info_changed',
      name: 'Migration plan',
    });
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({ sessionName: 'Migration plan' }),
      );
      expect(tau.canRenameSession.value).toBe(true);
    });
  });

  it('adopts a name Pi reports without being asked', async () => {
    const { tau, project, controller } = await setupNamedSession();

    emitRpc(controller, {
      type: 'session_info_changed',
      name: 'Named by an extension',
    });

    await vi.waitFor(() => {
      expect(tau.sessionTitle.value).toBe('Named by an extension');
      expect(tau.projectSessions(project)[0]?.title).toBe(
        'Named by an extension',
      );
    });
    expect(invoke).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({ sessionName: 'Named by an extension' }),
    );
  });

  it('reads the name back from Pi when it rejects a rename', async () => {
    const { tau, project, controller, session } = await setupNamedSession();
    emitRpc(controller, { type: 'session_info_changed', name: 'Named in Pi' });
    await vi.waitFor(() => {
      expect(controller.sessionName).toBe('Named in Pi');
    });

    await tau.renameSession('Named in Tau');
    expect(tau.sessionTitle.value).toBe('Named in Tau');
    expect(tau.projectSessions(project)[0]?.title).toBe('Named in Tau');
    emitRpc(controller, {
      id: sentRequests(controller, 'set_session_name')[0]?.id,
      type: 'response',
      command: 'set_session_name',
      success: false,
      error: { message: 'Session name cannot be empty' },
    });

    await vi.waitFor(() => {
      expect(controller.status).toBe('Session name cannot be empty');
      expect(sentRequests(controller, 'get_state')).toHaveLength(1);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state')[0]?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        sessionId: session.id,
        sessionFile: session.path,
        sessionName: 'Named in Pi',
        isStreaming: false,
      },
    });

    await vi.waitFor(() => {
      expect(tau.sessionTitle.value).toBe('Named in Pi');
      expect(tau.projectSessions(project)[0]?.title).toBe('Named in Pi');
    });
  });

  it('rolls the optimistic title back when sending the rename fails', async () => {
    const { tau, project, controller } = await setupNamedSession();
    const defaultInvoke = vi.mocked(invoke).getMockImplementation();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'send_pi') throw new Error('Pi is unavailable');
      return defaultInvoke?.(command, args);
    });

    try {
      await tau.renameSession('Name that cannot be sent');

      expect(controller.sessionName).toBe('naming-target');
      expect(tau.sessionTitle.value).toBe('naming-target');
      expect(tau.projectSessions(project)[0]?.title).toBe('naming-target');
      expect(controller.status).toBe('Pi is unavailable');
      expect(controller.pendingSessionRename).toBeUndefined();
    } finally {
      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
    }
  });

  it('leaves a session Pi has no file for unnamed', async () => {
    const { tau, project } = await setupNamedSession();
    await tau.newSession(project);
    const controller = tau.state.controllers.find(
      (candidate) => candidate.phantom,
    );
    if (!controller) throw new Error('Expected a phantom controller');

    expect(tau.canRenameSession.value).toBe(false);
    await tau.renameSession('Too early');
    expect(sentRequests(controller, 'set_session_name')).toEqual([]);
  });
});

describe('runtime retention', () => {
  it('keeps an idle runtime warm when the user switches away', async () => {
    const { project, sessions, tau } = await setupIdleRuntimes(2);

    await tau.selectSession(project, sessions[1]!);

    expect(stoppedRuntimes()).toEqual([]);
    tau.dispose();
  });

  it('releases the least recently active runtimes past the limit', async () => {
    const { project, sessions, tau } = await setupIdleRuntimes(8);
    const controllerFor = (id: string): SessionController | undefined =>
      tau.state.controllers.find((candidate) => candidate.sessionId === id);

    // Selecting one of the eight leaves seven idle runtimes for six places.
    await tau.selectSession(project, sessions[6]!);

    expect(stoppedRuntimes()).toEqual([controllerFor('idle-0')?.runtimeId]);
    expect(controllerFor('idle-1')?.ready).toBe(true);

    // Reading the oldest survivor makes it the most recent, so the next
    // sweep reaches past it to the one the user has not touched since.
    await tau.selectSession(project, sessions[1]!);
    await tau.newSession(project);

    expect(stoppedRuntimes()).toEqual([
      controllerFor('idle-0')?.runtimeId,
      controllerFor('idle-2')?.runtimeId,
    ]);
    expect(controllerFor('idle-1')?.ready).toBe(true);
    tau.dispose();
  });

  it('keeps a runtime that is still working, whatever its age', async () => {
    const { project, sessions, tau } = await setupIdleRuntimes(8);
    const oldest = tau.state.controllers.find(
      (candidate) => candidate.sessionId === 'idle-0',
    );
    if (!oldest) throw new Error('Expected the oldest controller');
    oldest.streaming = true;
    oldest.working = true;

    await tau.selectSession(project, sessions[6]!);

    expect(stoppedRuntimes()).toEqual([]);
    expect(oldest.ready).toBe(true);
    tau.dispose();
  });
});

function stoppedRuntimes(): string[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === 'stop_pi')
    .map(([, args]) => (args as { runtimeId: string }).runtimeId);
}

/** Open `count` sessions in order, leaving each one ready and idle. */
async function setupIdleRuntimes(count: number): Promise<{
  project: ProjectSummary;
  sessions: SessionSummary[];
  tau: Tau;
}> {
  const sessions = Array.from({ length: count }, (_, index) =>
    savedSession(`idle-${index}`),
  );
  const project: ProjectSummary = {
    path: '/tmp/tau-retention-test',
    name: 'tau-retention-test',
    workingDirectory: '/tmp/tau-retention-test',
    collapsed: false,
    selected: true,
    sessions,
  };
  const workspace: WorkspaceSnapshot = {
    activeProjectPath: project.path,
    piPath: '/usr/local/bin/pi',
    projects: [project],
  };
  mocks.workspace = workspace;
  mocks.generation = 0;

  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = '';
  tau.state.activeSessionId = '';
  tau.state.activeSessionPath = '';
  tau.state.activeControllerKey = '';
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  await tau.initialize();
  tau.state.workspace = workspace;

  // Selection order is the activity order the retention sweep reads. The
  // runtimes only report ready afterwards, so no sweep runs during setup.
  for (const session of sessions) await tau.selectSession(project, session);
  for (const controller of tau.state.controllers) {
    controller.starting = false;
    controller.ready = true;
    controller.streaming = false;
    controller.working = false;
  }
  vi.mocked(invoke).mockClear();

  return { project, sessions, tau };
}

/**
 * A session Tau registered from a workflow handoff, cancelled before it ever
 * answered, so Pi never wrote its file and only Tau's sidebar holds it.
 */
async function setupUnsavedSession(): Promise<{
  tau: Tau;
  project: ProjectSummary;
  ghost: SessionSummary;
  other: SessionSummary;
  controller: SessionController;
}> {
  const ghost = savedSession('unsaved-phase', 2);
  const other = savedSession('saved-session', 1);
  const project: ProjectSummary = {
    path: '/tmp/tau-unsaved-test',
    name: 'tau-unsaved-test',
    workingDirectory: '/tmp/tau-unsaved-test',
    collapsed: false,
    selected: false,
    sessions: [ghost, other],
  };
  const workspace: WorkspaceSnapshot = {
    activeProjectPath: project.path,
    piPath: '/usr/local/bin/pi',
    projects: [project],
  };
  mocks.workspace = workspace;
  mocks.generation = 0;

  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = '';
  tau.state.activeSessionId = '';
  tau.state.activeSessionPath = '';
  tau.state.activeControllerKey = '';
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  await tau.initialize();
  tau.state.workspace = workspace;

  await tau.selectSession(project, ghost);
  const controller = tau.state.controllers.find(
    (candidate) => candidate.sessionId === ghost.id,
  );
  if (!controller) throw new Error('Expected the unsaved session controller');
  vi.mocked(invoke).mockClear();
  return { tau, project, ghost, other, controller };
}

async function setupNamedSession(): Promise<{
  tau: Tau;
  project: ProjectSummary;
  session: SessionSummary;
  controller: SessionController;
}> {
  const session = savedSession('naming-target');
  const project: ProjectSummary = {
    path: '/tmp/tau-naming-test',
    name: 'tau-naming-test',
    workingDirectory: '/tmp/tau-naming-test',
    collapsed: false,
    selected: true,
    sessions: [session],
  };
  const workspace: WorkspaceSnapshot = {
    activeProjectPath: project.path,
    piPath: '/usr/local/bin/pi',
    projects: [project],
  };
  mocks.workspace = workspace;
  mocks.generation = 0;

  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = '';
  tau.state.activeSessionId = '';
  tau.state.activeSessionPath = '';
  tau.state.activeControllerKey = '';
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  await tau.initialize();
  tau.state.workspace = workspace;

  await tau.selectSession(project, session);
  const controller = tau.state.controllers.find(
    (candidate) => candidate.sessionId === session.id,
  );
  if (!controller) throw new Error('Expected the named session controller');
  controller.starting = false;
  controller.ready = true;
  vi.mocked(invoke).mockClear();
  return { tau, project, session, controller };
}

function emitRpc(
  controller: { runtimeId: string; generation: number },
  value: unknown,
): void {
  mocks.listener?.({
    payload: {
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'rpc',
      line: JSON.stringify(value),
    },
  });
}

function emitBridge(
  runtimeId: string,
  generation: number,
  kind: PiBridgeEvent['kind'],
  code?: number,
): void {
  mocks.listener?.({
    payload: { runtimeId, generation, kind, code },
  });
}

function emitTextDelta(
  controller: { runtimeId: string; generation: number },
  delta: string,
): void {
  mocks.listener?.({
    payload: {
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'rpc',
      line: JSON.stringify({
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta },
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
        command === 'send_pi' &&
        (args as { runtimeId?: string })?.runtimeId === controller.runtimeId,
    )
    .map(([, args]) => (args as { request: Record<string, unknown> }).request)
    .filter((request) => request.type === type);
}

async function setupExtensionControllers(): Promise<{
  tau: Tau;
  project: ProjectSummary;
  firstSession: SessionSummary;
  secondSession: SessionSummary;
  firstController: SessionController;
  secondController: SessionController;
}> {
  const firstSession = savedSession('first');
  const secondSession = savedSession('second');
  const project: ProjectSummary = {
    path: '/tmp/extension-ui-test',
    name: 'extension-ui-test',
    workingDirectory: '/tmp/extension-ui-test',
    collapsed: false,
    selected: true,
    sessions: [firstSession, secondSession],
  };
  const workspace: WorkspaceSnapshot = {
    activeProjectPath: project.path,
    piPath: '/usr/local/bin/pi',
    projects: [project],
  };
  mocks.workspace = workspace;
  mocks.generation = 0;

  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = '';
  tau.state.activeSessionId = '';
  tau.state.activeSessionPath = '';
  tau.state.activeControllerKey = '';
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  await tau.initialize();
  tau.state.workspace = workspace;

  await tau.selectSession(project, firstSession);
  const firstController = tau.state.controllers.find(
    (controller) => controller.sessionId === firstSession.id,
  );
  if (!firstController) throw new Error('Expected the first controller');
  firstController.starting = false;
  firstController.ready = true;
  firstController.streaming = true;
  firstController.working = true;

  await tau.selectSession(project, secondSession);
  const secondController = tau.state.controllers.find(
    (controller) => controller.sessionId === secondSession.id,
  );
  if (!secondController) throw new Error('Expected the second controller');
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

/** Runs the settle Pi performs after a turn: state first, then its messages. */
async function settleWith(
  controller: { runtimeId: string; generation: number },
  session: { id: string; path: string; title: string },
  messages: unknown[],
): Promise<void> {
  const before = sentRequests(controller, 'get_state').length;
  emitRpc(controller, { type: 'agent_settled' });
  await vi.waitFor(() => {
    expect(sentRequests(controller, 'get_state')).toHaveLength(before + 1);
  });
  emitRpc(controller, {
    id: sentRequests(controller, 'get_state')[before]?.id,
    type: 'response',
    command: 'get_state',
    success: true,
    data: {
      model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
      thinkingLevel: 'high',
      sessionId: session.id,
      sessionFile: session.path,
      sessionName: session.title,
      isStreaming: false,
    },
  });

  await vi.waitFor(() => {
    expect(sentRequests(controller, 'get_messages')).toHaveLength(1);
  });
  emitRpc(controller, {
    id: sentRequests(controller, 'get_messages')[0]?.id,
    type: 'response',
    command: 'get_messages',
    success: true,
    data: { messages },
  });
  await vi.waitFor(() => {
    expect(controllerOf(controller).syncing).toBe(false);
  });
}

function controllerOf(controller: unknown): { syncing: boolean } {
  return controller as { syncing: boolean };
}

function savedSession(id: string, lastUserMessageAt = 0): SessionSummary {
  return {
    id,
    path: `/tmp/${id}.jsonl`,
    title: id,
    lastActive: 'now',
    lastUserMessageAt,
    archived: false,
    selected: false,
  };
}
