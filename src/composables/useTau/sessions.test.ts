import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { PiBridgeEvent } from '../../lib/pi/bridge';
import { nextRequestId, state } from '../state';
import type {
  ProjectSummary,
  SessionController,
  SessionSummary,
  WorkspaceSnapshot,
} from '../state';

import useTau from './index';

type Tau = ReturnType<typeof useTau>;

function feedbackMessage(controller: SessionController): string {
  return (
    [...controller.feedback]
      .reverse()
      .find((incident) => !incident.acknowledged)?.message ?? ''
  );
}

const mocks = vi.hoisted(() => ({
  workspace: null as WorkspaceSnapshot | null,
  modelScope: [] as string[],
  generation: 0,
  listener: undefined as
    ((event: { payload: PiBridgeEvent }) => void) | undefined,
  projectSelection: null as string | null,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(
    async (
      command: string,
      args?: { sessionId?: string; projectPath?: string; path?: string },
    ) => {
      if (command === 'read_pi_frontend_revision') return 0;
      if (command === 'claim_pi_frontend') return undefined;
      if (command.startsWith('start_pi')) {
        mocks.generation += 1;
        return mocks.generation;
      }
      if (command.endsWith('model_scope')) {
        return mocks.modelScope;
      }
      if (command === 'import_project' || command === 'import_remote_project') {
        return (
          mocks.workspace?.projects.find(
            (project) => project.path === args?.path,
          ) ?? mocks.workspace?.projects[0]
        );
      }
      if (command === 'register_session') {
        return (
          mocks.workspace?.projects
            .find((project) => project.path === args?.projectPath)
            ?.sessions.find((session) => session.id === args?.sessionId) ?? null
        );
      }
      if (command === 'archive_session' || command === 'unarchive_session') {
        const project = mocks.workspace?.projects.find(
          (project) => project.path === args?.projectPath,
        );
        const session = project?.sessions.find(
          (session) => session.id === args?.sessionId,
        );
        if (session) session.archived = command === 'archive_session';
        return {
          sessionId: args?.sessionId,
          archived: command === 'archive_session',
          activeSessionId: '',
        };
      }
      return mocks.workspace;
    },
  ),
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

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(async () => mocks.projectSelection),
}));

beforeEach(() => {
  state.workspaceFeedback = [];
});

describe('session drafts and selection', () => {
  it('opens a usable new session when the first local project is imported', async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-first-project',
      name: 'tau-first-project',
      workingDirectory: '/tmp/tau-first-project',
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const importedWorkspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: '/opt/homebrew/bin/pi',
      projects: [project],
    };
    mocks.workspace = importedWorkspace;
    mocks.projectSelection = project.path;
    mocks.generation = 0;
    vi.mocked(invoke).mockClear();

    const { state, addLocalProject, canDraft } = useTau();
    state.activeProjectPath = '';
    state.activeSessionId = '';
    state.activeSessionPath = '';
    state.activeControllerKey = '';
    state.controllers.splice(0);
    state.ephemeralSessions.splice(0);
    state.workspace = { activeProjectPath: '', piPath: null, projects: [] };

    await addLocalProject();

    expect(canDraft.value).toBe(true);
    expect(state.activeProjectPath).toBe(project.path);
    expect(state.activeSessionId).toMatch(/^phantom-/);
    expect(state.controllers).toHaveLength(1);
    expect(
      vi.mocked(invoke).mock.calls.some(([command]) => command === 'start_pi'),
    ).toBe(true);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === 'send_pi' &&
            (args as { request: { type: string } }).request.type ===
              'get_available_models',
        ),
    ).toBe(true);
    mocks.projectSelection = null;
  });

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
      sessionLastActive,
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
    expect(first?.title).toBe('New Session');
    expect(first && sessionLastActive(project, first)).toBe('');

    draft.value = '  Keep this draft\nwith its session  ';
    expect(first?.title).toBe('Keep this draft with its session');
    expect(first && sessionIndicator(project, first)).toBe('draft');
    expect(first && sessionLastActive(project, first)).toBe('');

    const firstController = state.controllers.find(
      (controller) => controller.sessionId === first?.id,
    );
    if (!first || !firstController) {
      throw new Error('Expected the first phantom session');
    }
    firstController.messages.push({
      id: 'assistant-1',
      kind: 'assistant',
      text: 'A persisted reply',
    });
    expect(sessionLastActive(project, first)).toBe('now');
    firstController.streaming = true;
    firstController.working = true;

    await newSession(project);
    expect(projectSessions(project)).toHaveLength(2);
    expect(sessionIndicator(project, first)).toBe('working');

    const empty = projectSessions(project)[0];
    expect(empty?.title).toBe('New Session');
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
    const startupCommands = vi
      .mocked(invoke)
      .mock.calls.map(([command]) => command);
    expect(startupCommands.indexOf('claim_pi_frontend')).toBeLessThan(
      startupCommands.indexOf('load_workspace'),
    );
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
    expect(startupCommands.indexOf('claim_pi_frontend')).toBeLessThan(
      vi
        .mocked(invoke)
        .mock.calls.findIndex(([command]) => command === 'start_pi'),
    );
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
      expect(
        sentRequests(controller, 'get_messages').some(
          (request) => request.id === controller.startMessagesRequestId,
        ),
      ).toBe(true);
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

    emitRpc(controller, {
      type: 'extension_ui_request',
      id: 'tooltip-draft',
      method: 'set_editor_text',
      text: 'Extension *draft*\n\n- item',
    });
    expect(
      projectSessions(project).find(
        (session) => session.id === controller.sessionId,
      )?.titleMarkdown,
    ).toBe('Extension *draft*\n\n- item');
    draft.value = '**Start**\n\n  background work';
    expect(
      projectSessions(project).find(
        (session) => session.id === controller.sessionId,
      )?.titleMarkdown,
    ).toBe(draft.value);
    await sendMessage();
    expect(controller.promptSubmitting).toBe(true);
    expect(draft.value).toBe('');
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
      expect(
        projectSessions(project).find(
          (session) => session.id === 'materialized',
        )?.titleMarkdown,
      ).toBe('**Start**\n\n  background work');
      expect(controller.pendingPrompt?.messagesRequestId).not.toBe('');
      expect(
        sentRequests(controller, 'get_messages').some(
          (request) =>
            request.id === controller.pendingPrompt?.messagesRequestId,
        ),
      ).toBe(true);
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
      expect(controller.promptSubmitting).toBe(false);
    });
    expect(draft.value).toBe('');

    await newSession(project);

    expect(
      projectSessions(project).some((session) => session.id === 'materialized'),
    ).toBe(true);
  });

  it('removes a failed empty phantom after its restored draft is cleared', async () => {
    const main = savedSession('main');
    const project: ProjectSummary = {
      path: 'ssh:failed-phantom',
      name: 'failed-phantom',
      workingDirectory: '/remote/project',
      connectionString: 'fixture@example',
      collapsed: false,
      selected: true,
      sessions: [main],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: null,
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
    tau.state.workspace = workspace;

    await tau.newSession(project);
    const controller = tau.state.controllers[0];
    const ephemeral = tau.state.ephemeralSessions[0];
    if (!controller || !ephemeral) {
      throw new Error('Expected a fresh phantom session');
    }
    controller.starting = false;
    controller.connectingRemote = false;
    controller.ready = false;
    vi.mocked(invoke).mockClear();

    const defaultInvoke = vi.mocked(invoke).getMockImplementation();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'start_pi_remote') {
        throw new Error('Connection failed');
      }
      return defaultInvoke?.(command, args);
    });

    try {
      const submittedDraft = '  Keep this exact draft  ';
      for (let attempt = 0; attempt < 3; attempt += 1) {
        tau.draft.value = submittedDraft;
        await tau.sendMessage();

        expect(tau.draft.value).toBe(submittedDraft);
        expect(controller.messages).toEqual([]);
        expect(controller.pendingPrompt).toBeUndefined();
        expect(controller.submittedPrompt).toBeUndefined();
        expect(controller.promptSubmitting).toBe(false);
        expect(controller.starting).toBe(false);
        expect(controller.working).toBe(false);
        expect(controller.lastUserMessageAt).toBe(0);
        expect(ephemeral.lastUserMessageAt).toBe(0);
        expect(sentRequests(controller, 'prompt')).toEqual([]);
      }

      await tau.selectSession(project, main);
      expect(tau.state.ephemeralSessions).toContain(ephemeral);
      expect(tau.state.controllers).toContain(controller);

      await tau.selectSession(project, ephemeral);
      tau.draft.value = '';
      await tau.selectSession(project, main);

      expect(tau.state.activeSessionId).toBe(main.id);
      expect(tau.state.ephemeralSessions).not.toContain(ephemeral);
      expect(tau.state.controllers).not.toContain(controller);
      expect(controller.disposed).toBe(true);
      expect(tau.state.controllers).toHaveLength(1);
    } finally {
      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
      tau.dispose();
    }
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

    expect(projectSessions(project)[0]?.title).toBe('New Session');
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

  it('does not archive a session while its controller is streaming', async () => {
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
    ).toBe(false);
    expect(projectSessions(project)).toEqual([firstSession, secondSession]);
    expect(state.activeSessionId).toBe(firstSession.id);
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

  it('sorts user-message-less sessions by their first agent message', async () => {
    const project: ProjectSummary = {
      path: '/tmp/tau-agent-order-test',
      name: 'tau-agent-order-test',
      workingDirectory: '/tmp/tau-agent-order-test',
      collapsed: false,
      selected: true,
      sessions: [
        savedSession('older-agent-session', 0, 1_000),
        savedSession('user-session', 2_000),
        savedSession('newer-agent-session', 0, 3_000),
      ],
    };
    mocks.workspace = {
      activeProjectPath: project.path,
      piPath: '/usr/local/bin/pi',
      projects: [project],
    };
    const { initialize, projectSessions } = useTau();

    await initialize();

    expect(projectSessions(project).map((session) => session.id)).toEqual([
      'newer-agent-session',
      'user-session',
      'older-agent-session',
    ]);
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
      expect(
        sentRequests(controller, 'get_messages').some(
          (request) => request.id === controller.startMessagesRequestId,
        ),
      ).toBe(true);
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
    emitRpc(firstController, {
      type: 'message_start',
      message: { role: 'user', content: 'Review the plan' },
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
    expect(firstController.messages).toMatchObject([
      { kind: 'user', text: 'Review the plan' },
    ]);
    expect(firstController.commandsLoaded).toBe(false);
    expect(tau.state.activeControllerKey).toBe(secondController.key);
    expect(tau.state.activeSessionId).toBe(secondSession.id);
    expect(
      tau.state.workspace?.projects[0]?.sessions.map((session) => session.id),
    ).toEqual([firstSession.id, secondSession.id]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === 'register_session' &&
            (args as { sessionId?: string })?.sessionId ===
              replacementSession.id,
        ),
    ).toBe(false);
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
        { id: 'stream-user-0', kind: 'user', text: 'Review the plan' },
      ]);
    });
    await settleAndHydrateCompleted(firstController, 'Plan ready');
    await vi.waitFor(() => {
      expect(
        tau.state.workspace?.projects[0]?.sessions.map((session) => session.id),
      ).toEqual([firstSession.id, secondSession.id, replacementSession.id]);
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
    emitRpc(secondController, {
      type: 'message_start',
      message: { role: 'assistant', content: [] },
    });

    await vi.waitFor(() => {
      expect(secondController.sessionId).toBe(phaseSession.id);
    });
    expect(tau.state.activeSessionId).toBe(phaseSession.id);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(
          ([command, args]) =>
            command === 'register_session' &&
            (args as { sessionId?: string })?.sessionId === phaseSession.id,
        ),
    ).toBe(false);

    await settleAndHydrateCompleted(secondController, 'Phase ready');
    await vi.waitFor(() => {
      expect(tau.state.activeSessionId).toBe(phaseSession.id);
    });
    expect(secondController.messages.map((message) => message.text)).toEqual([
      'Prompt',
      'Phase ready',
    ]);
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
      expect(
        sentRequests(controller, 'get_messages').some(
          (request) =>
            request.id === controller.pendingPrompt?.messagesRequestId,
        ),
      ).toBe(true);
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
    emitRpc(controller, {
      type: 'message_start',
      message: { role: 'assistant', content: [] },
    });
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === 'register_session'),
    ).toHaveLength(0);

    await settleAndHydrateCompleted(controller, 'Phase ready');
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

    const registered = tau.state.workspace?.projects[0];
    if (!registered) throw new Error('Expected the registered project');
    const titles = tau.projectSessions(registered).map((item) => item.title);
    expect(titles).toContain('42 • plan');
    expect(titles).not.toContain('/mock 42');
    tau.dispose();
  });

  it('keeps an unanswered command and its runtime when leaving', async () => {
    const { controller, otherSession, project, tau } =
      await setupUnansweredPhantomCommand();
    const session = tau.state.ephemeralSessions.find(
      (candidate) => candidate.controllerKey === controller.key,
    );
    if (!session) throw new Error('Expected the command session');

    await tau.archiveSession(project, session);
    expect(tau.state.ephemeralSessions).toContain(session);
    expect(tau.state.controllers).toContain(controller);
    expect(tau.state.activeSessionId).toBe(session.id);
    expect(
      vi.mocked(invoke).mock.calls.some(([name]) => name === 'archive_session'),
    ).toBe(false);

    await tau.selectSession(project, otherSession);

    expect(tau.state.ephemeralSessions).toContain(session);
    expect(tau.state.controllers).toContain(controller);
    expect(tau.state.activeSessionId).toBe(otherSession.id);
    expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
    tau.dispose();
  });

  it.each([false, true])(
    'keeps a hidden command through reconciliation, then handles an empty replacement: %s',
    async (replacement) => {
      const { controller, otherSession, project, tau } =
        await setupUnansweredPhantomCommand();
      const session = tau.state.ephemeralSessions.find(
        (candidate) => candidate.controllerKey === controller.key,
      );
      if (!session) throw new Error('Expected the command session');

      // An unrelated idle poll cannot signal that the extension handler finished.
      const { rpc } = await import('../../lib/pi/runtime');
      const idleId = nextRequestId('idle-state');
      await rpc(controller, { id: idleId, type: 'get_state' });
      emitRpc(controller, {
        id: idleId,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          isStreaming: false,
        },
      });
      await tau.selectSession(project, otherSession);
      expect(tau.state.ephemeralSessions).toContain(session);
      expect(stoppedRuntimes()).not.toContain(controller.runtimeId);

      emitRpc(controller, {
        id: controller.commandPromptRequestId,
        type: 'response',
        command: 'prompt',
        success: true,
      });
      await vi.waitFor(() =>
        expect(controller.commandSyncRequestId).not.toBe(''),
      );
      await tau.selectSession(project, session);
      await tau.selectSession(project, otherSession);
      expect(tau.state.ephemeralSessions).toContain(session);
      expect(stoppedRuntimes()).not.toContain(controller.runtimeId);

      emitRpc(controller, {
        id: controller.commandSyncRequestId,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          sessionId: replacement ? 'command-successor' : controller.sessionId,
          sessionFile: replacement
            ? '/tmp/command-successor.jsonl'
            : controller.sessionPath,
          isStreaming: false,
        },
      });
      await vi.waitFor(() =>
        expect(controller.commandMessagesRequestId).not.toBe(''),
      );
      expect(controller.sessionId).toBe(
        replacement ? 'command-successor' : session.id,
      );
      await tau.newSession(project);
      expect(tau.state.ephemeralSessions).toContain(session);
      expect(stoppedRuntimes()).not.toContain(controller.runtimeId);

      emitRpc(controller, {
        id: controller.commandMessagesRequestId,
        type: 'response',
        command: 'get_messages',
        success: true,
        data: { messages: [] },
      });
      await vi.waitFor(() =>
        expect(controller.commandMessagesRequestId).toBe(''),
      );
      expect(tau.state.ephemeralSessions).toContain(session);
      await tau.selectSession(project, session);
      await tau.selectSession(project, otherSession);
      if (replacement) {
        expect(tau.state.ephemeralSessions).toContain(session);
        expect(tau.state.controllers).toContain(controller);
        expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
      } else {
        expect(tau.state.ephemeralSessions).not.toContain(session);
      }
      tau.dispose();
    },
  );

  it('keeps an empty command row when its history read fails', async () => {
    const { controller, otherSession, project, tau } =
      await setupUnansweredPhantomCommand();
    const session = tau.state.ephemeralSessions.find(
      (candidate) => candidate.controllerKey === controller.key,
    );
    if (!session) throw new Error('Expected the command session');
    emitRpc(controller, {
      id: controller.commandPromptRequestId,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() =>
      expect(controller.commandSyncRequestId).not.toBe(''),
    );
    emitRpc(controller, {
      id: controller.commandSyncRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: false,
      },
    });
    await vi.waitFor(() =>
      expect(controller.commandMessagesRequestId).not.toBe(''),
    );
    emitRpc(controller, {
      id: controller.commandMessagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: false,
    });
    await vi.waitFor(() => expect(controller.commandRefreshFailed).toBe(true));
    await tau.selectSession(project, otherSession);
    expect(tau.state.ephemeralSessions).toContain(session);
    expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
    tau.dispose();
  });

  it.each(['failure', 'timeout'] as const)(
    'settles successor loading after command history %s, even after feedback dismissal',
    async (terminal) => {
      const { controller, tau } = await setupUnansweredPhantomCommand();
      if (terminal === 'timeout') vi.useFakeTimers();
      emitRpc(controller, {
        id: controller.commandPromptRequestId,
        type: 'response',
        command: 'prompt',
        success: true,
      });
      if (terminal === 'timeout') await vi.advanceTimersByTimeAsync(0);
      else
        await vi.waitFor(() =>
          expect(controller.commandSyncRequestId).not.toBe(''),
        );
      emitRpc(controller, {
        id: controller.commandSyncRequestId,
        type: 'response',
        command: 'get_state',
        success: true,
        data: {
          sessionId: 'empty-successor',
          sessionFile: '/tmp/empty-successor.jsonl',
          isStreaming: false,
        },
      });
      if (terminal === 'timeout') await vi.advanceTimersByTimeAsync(0);
      else
        await vi.waitFor(() =>
          expect(controller.commandMessagesRequestId).not.toBe(''),
        );
      expect(controller.syncing).toBe(true);
      if (terminal === 'failure') {
        emitRpc(controller, {
          id: controller.commandMessagesRequestId,
          type: 'response',
          command: 'get_messages',
          success: false,
        });
        await vi.waitFor(() =>
          expect(controller.commandRefreshFailed).toBe(true),
        );
      } else {
        await vi.advanceTimersByTimeAsync(10_001);
        vi.useRealTimers();
      }
      expect(controller.commandRefreshFailed).toBe(true);
      expect(controller.syncing).toBe(false);
      expect(tau.activeFeedback.value).toBeDefined();
      tau.acknowledgeFeedback(tau.activeFeedback.value!);
      expect(tau.sessionLoading.value).toBe(false);
      expect(controller.commandMessagesRequestId).toBe('');
      tau.dispose();
    },
  );

  it('transfers command reconciliation to a newer history read', async () => {
    const { controller, tau } = await setupUnansweredPhantomCommand();
    emitRpc(controller, {
      id: controller.commandPromptRequestId,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() =>
      expect(controller.commandSyncRequestId).not.toBe(''),
    );
    emitRpc(controller, {
      id: controller.commandSyncRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: false,
      },
    });
    await vi.waitFor(() =>
      expect(controller.commandMessagesRequestId).not.toBe(''),
    );
    const olderRead = controller.commandMessagesRequestId;
    const newerRead = nextRequestId('newer-messages');
    const { rpc } = await import('../../lib/pi/runtime');
    await rpc(controller, { id: newerRead, type: 'get_messages' });
    expect(controller.commandMessagesRequestId).toBe(newerRead);
    emitRpc(controller, {
      id: olderRead,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    expect(controller.commandMessagesRequestId).toBe(newerRead);
    emitRpc(controller, {
      id: newerRead,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() =>
      expect(controller.commandMessagesRequestId).toBe(''),
    );
    expect(controller.commandRefreshFailed).toBe(false);
    tau.dispose();
  });

  it('retains a failed command refresh and retries reads without resending the command', async () => {
    const { controller, otherSession, project, tau } =
      await setupUnansweredPhantomCommand();
    const session = tau.state.ephemeralSessions.find(
      (candidate) => candidate.controllerKey === controller.key,
    );
    if (!session) throw new Error('Expected the command session');
    emitRpc(controller, {
      id: controller.commandPromptRequestId,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() =>
      expect(controller.commandSyncRequestId).not.toBe(''),
    );
    const firstRead = controller.commandSyncRequestId;
    emitRpc(controller, {
      id: firstRead,
      type: 'response',
      command: 'get_state',
      success: false,
    });
    await vi.waitFor(() => expect(controller.commandRefreshFailed).toBe(true));
    await tau.selectSession(project, otherSession);
    expect(tau.state.ephemeralSessions).toContain(session);
    expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
    await tau.selectSession(project, session);
    expect(controller.commandSyncRequestId).not.toBe(firstRead);
    expect(controller.commandSyncRequestId).not.toBe('');
    expect(sentRequests(controller, 'prompt')).toHaveLength(1);
    expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
    tau.dispose();
  });

  it('keeps a failed-refresh row after process loss rather than treating it as empty', async () => {
    const { controller, otherSession, project, tau } =
      await setupUnansweredPhantomCommand();
    const session = tau.state.ephemeralSessions.find(
      (candidate) => candidate.controllerKey === controller.key,
    );
    if (!session) throw new Error('Expected the command session');
    emitRpc(controller, {
      id: controller.commandPromptRequestId,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() =>
      expect(controller.commandSyncRequestId).not.toBe(''),
    );
    emitRpc(controller, {
      id: controller.commandSyncRequestId,
      type: 'response',
      command: 'get_state',
      success: false,
    });
    await vi.waitFor(() => expect(controller.commandRefreshFailed).toBe(true));
    await tau.selectSession(project, otherSession);
    const { handleBridgeEvent } = await import('../../lib/pi/runtime');
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 1,
    });
    expect(tau.state.ephemeralSessions).toContain(session);
    expect(controller.commandRefreshFailed).toBe(true);
    tau.dispose();
  });

  it('bounds command read reconciliation without timing out the handler', async () => {
    const { controller, otherSession, project, tau } =
      await setupUnansweredPhantomCommand();
    const session = tau.state.ephemeralSessions.find(
      (candidate) => candidate.controllerKey === controller.key,
    );
    if (!session) throw new Error('Expected the command session');
    vi.useFakeTimers();
    try {
      await vi.advanceTimersByTimeAsync(10_001);
      expect(controller.commandPromptRequestId).not.toBe('');
      expect(controller.commandRefreshFailed).toBe(false);
      await tau.selectSession(project, otherSession);
      expect(tau.state.ephemeralSessions).toContain(session);
      emitRpc(controller, {
        id: controller.commandPromptRequestId,
        type: 'response',
        command: 'prompt',
        success: true,
      });
      await vi.advanceTimersByTimeAsync(0);
      const lateRequestId = controller.commandSyncRequestId;
      expect(lateRequestId).not.toBe('');
      await vi.advanceTimersByTimeAsync(10_001);
      expect(controller.commandRefreshFailed).toBe(true);
      expect(controller.commandSyncRequestId).toBe('');
      expect(tau.state.ephemeralSessions).toContain(session);
      expect(stoppedRuntimes()).not.toContain(controller.runtimeId);
      emitRpc(controller, {
        id: lateRequestId,
        type: 'response',
        command: 'get_state',
        success: true,
        data: { sessionId: 'stale', sessionFile: '/tmp/stale.jsonl' },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(controller.sessionId).not.toBe('stale');
    } finally {
      vi.useRealTimers();
      tau.dispose();
    }
  });

  it.each([
    ['settled state', 'get_state'],
    ['effort refresh', 'get_available_thinking_levels'],
    ['settled messages', 'get_messages'],
  ])(
    'retries materialization after %s transport rejection',
    async (_label, rejectedMethod) => {
      const { firstController, firstSession, tau } =
        await setupExtensionControllers();
      let rejected = false;

      await vi.mocked(invoke).withImplementation(
        async (command, args) => {
          const request = (
            args as { request?: Record<string, unknown> } | undefined
          )?.request;
          if (
            command === 'send_pi' &&
            request?.type === rejectedMethod &&
            !rejected
          ) {
            rejected = true;
            throw new Error('transport rejected');
          }
          if (command.startsWith('start_pi')) {
            mocks.generation += 1;
            return mocks.generation;
          }
          if (command.endsWith('model_scope')) return mocks.modelScope;
          return mocks.workspace;
        },
        async () => {
          emitRpc(firstController, { type: 'agent_settled' });
          if (rejectedMethod !== 'get_state') {
            await vi.waitFor(() => {
              expect(firstController.materializationStateRequestId).not.toBe(
                '',
              );
            });
            emitRpc(firstController, {
              id: firstController.materializationStateRequestId,
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
          }
          await vi.waitFor(() => {
            expect(rejected).toBe(true);
            expect(firstController.materializationStateRequestId).toBe('');
            expect(firstController.materializationMessagesRequestId).toBe('');
            expect(firstController.syncing).toBe(false);
          });
          expect(firstController.postSettlementHydration).toBe(true);

          const { probeSessionReplacement } =
            await import('../../lib/pi/runtime');
          await probeSessionReplacement(firstController, false);
          const probeRequestId = firstController.replacementProbeRequestId;
          expect(probeRequestId).not.toBe('');
          emitRpc(firstController, {
            id: probeRequestId,
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
            expect(firstController.materializationMessagesRequestId).not.toBe(
              '',
            );
          });
          emitRpc(firstController, {
            id: firstController.materializationMessagesRequestId,
            type: 'response',
            command: 'get_messages',
            success: true,
            data: {
              messages: [
                {
                  role: 'assistant',
                  content: [{ type: 'text', text: 'Durable after retry' }],
                },
              ],
            },
          });
          await vi.waitFor(() => {
            expect(firstController.materializationVerified).toBe(true);
            expect(firstController.postSettlementHydration).toBe(false);
          });
        },
      );
      tau.dispose();
    },
  );

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
      emitRpc(firstController, {
        type: 'message_start',
        message: { role: 'user', content: 'Execute the plan' },
      });
      await vi.waitFor(() => {
        expect(firstController.sessionId).toBe(phaseSession.id);
        expect(sentRequests(firstController, 'get_messages')).toHaveLength(2);
      });
      emitRpc(firstController, {
        id: sentRequests(firstController, 'get_messages')[1]?.id,
        type: 'response',
        command: 'get_messages',
        success: true,
        data: {
          messages: [
            { role: 'user', content: 'Execute the plan' },
            {
              role: 'assistant',
              content: [{ type: 'text', text: 'Execution complete' }],
            },
          ],
        },
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

  it('still retires a legacy stale row instead of opening a copy of it', async () => {
    const { controller, ghost, other, project, tau } =
      await setupUnsavedSession();

    // Pi answers --session for a file it cannot find by opening a fresh

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
    expect(rows.map((row) => row.title)).toEqual(['New Session', other.title]);
    expect(tau.state.activeSessionId).toBe(rows[0]?.id);
    expect(feedbackMessage(controller)).toBe(
      'The previous session is unavailable. Enter a message to continue in this new session.',
    );

    emitRpc(controller, {
      id: sentRequests(controller, 'get_messages')[0]?.id,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });

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
    emitRpc(controller, {
      type: 'message_start',
      message: { role: 'assistant', content: [] },
    });

    // until a settled hydration confirms Pi appended the assistant message.
    expect(controller.phantom).toBe(false);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === 'register_session'),
    ).toBe(false);
    await settleAndHydrateCompleted(controller, 'Phase ready');
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
  it('keeps a response draft in place until Pi accepts it', async () => {
    const { firstController, firstSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'pending-response',
      method: 'input',
      title: 'Issue ID',
    });
    const dialog = tau.activeExtensionDialog.value;
    if (!dialog) throw new Error('Expected the extension prompt');
    dialog.draft = 'ENG-42';

    const defaultInvoke = vi.mocked(invoke).getMockImplementation();
    let rejectSend: ((error: Error) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const request = (args as { request?: { type?: string } })?.request;
      if (command === 'send_pi' && request?.type === 'extension_ui_response') {
        await new Promise<never>((_resolve, reject) => {
          rejectSend = reject;
        });
      }
      return defaultInvoke?.(command, args);
    });

    try {
      const submission = tau.submitExtensionDialog(dialog.draft);
      await vi.waitFor(() => {
        expect(dialog.submitting).toBe(true);
      });
      expect(dialog.draft).toBe('ENG-42');

      rejectSend?.(new Error('Pi is unavailable'));
      await submission;

      expect(tau.activeExtensionDialog.value).toBe(dialog);
      expect(dialog.submitting).toBe(false);
      expect(dialog.draft).toBe('ENG-42');
      expect(dialog.error).toBe('The response could not be sent. Try again.');

      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
      await tau.submitExtensionDialog(dialog.draft);
      expect(tau.activeExtensionDialog.value).toBeUndefined();
    } finally {
      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
      tau.dispose();
    }
  });

  it('keeps a failed cancellation retryable without duplicate sends or composer loss', async () => {
    const { firstController, firstSession, project, tau } =
      await setupExtensionControllers();
    await tau.selectSession(project, firstSession);
    tau.draft.value = 'Keep the normal message draft';
    emitRpc(firstController, {
      type: 'extension_ui_request',
      id: 'cancel-retry',
      method: 'editor',
      title: 'Release notes',
      prefill: 'Unfinished prompt answer',
    });
    const dialog = tau.activeExtensionDialog.value;
    if (!dialog) throw new Error('Expected the extension prompt');

    const defaultInvoke = vi.mocked(invoke).getMockImplementation();
    let rejectSend: ((error: Error) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const request = (args as { request?: { type?: string } })?.request;
      if (command === 'send_pi' && request?.type === 'extension_ui_response') {
        await new Promise<never>((_resolve, reject) => {
          rejectSend = reject;
        });
      }
      return defaultInvoke?.(command, args);
    });

    try {
      const cancellation = tau.cancelExtensionDialog();
      await vi.waitFor(() => expect(dialog.submitting).toBe(true));
      await tau.cancelExtensionDialog();
      expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
        {
          type: 'extension_ui_response',
          id: 'cancel-retry',
          cancelled: true,
        },
      ]);

      rejectSend?.(new Error('Pi is unavailable'));
      await cancellation;
      expect(tau.activeExtensionDialog.value).toBe(dialog);
      expect(dialog).toMatchObject({
        draft: 'Unfinished prompt answer',
        submitting: false,
        error: 'The response could not be sent. Try again.',
      });
      expect(tau.draft.value).toBe('Keep the normal message draft');

      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
      await tau.cancelExtensionDialog();
      expect(tau.activeExtensionDialog.value).toBeUndefined();
      expect(tau.draft.value).toBe('Keep the normal message draft');
      expect(sentRequests(firstController, 'extension_ui_response')).toEqual([
        {
          type: 'extension_ui_response',
          id: 'cancel-retry',
          cancelled: true,
        },
        {
          type: 'extension_ui_response',
          id: 'cancel-retry',
          cancelled: true,
        },
      ]);
    } finally {
      if (defaultInvoke) vi.mocked(invoke).mockImplementation(defaultInvoke);
      tau.dispose();
    }
  });

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
    firstController.feedback.push({
      id: 1,
      title: 'Connection Warning',
      message: 'Tau connection warning',
      acknowledged: false,
      controllerKey: firstController.key,
    });

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
    expect(feedbackMessage(firstController)).toBe('Tau connection warning');
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

    const { rpc } = await import('../../lib/pi/runtime');
    const hydrationRequestId = nextRequestId('messages');
    await rpc(firstController, {
      id: hydrationRequestId,
      type: 'get_messages',
    });
    emitRpc(firstController, {
      type: 'response',
      id: hydrationRequestId,
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
    expect(tau.activeFeedback.value?.message ?? '').toBe(
      'Tau connection warning',
    );
    expect(tau.messages.value).toBe(firstController.messages);
    tau.dispose();
  });

  it('prioritizes collapsed project indicators by urgency', async () => {
    const { firstController, secondController, project, tau } =
      await setupExtensionControllers();

    project.collapsed = true;
    secondController.draft = 'Keep this draft';
    expect(tau.projectIndicator(project)).toBe('working');

    secondController.draft = '';
    secondController.unread = true;
    expect(tau.projectIndicator(project)).toBe('new');

    secondController.unread = false;
    expect(tau.projectIndicator(project)).toBe('working');

    firstController.working = false;
    secondController.draft = 'Keep this draft';
    expect(tau.projectIndicator(project)).toBe('draft');

    secondController.draft = '';
    expect(tau.projectIndicator(project)).toBe('');
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
    expect(tau.sessionIndicator(project, firstSession)).toBe('new');
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

function stoppedRuntimes(): string[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === 'stop_pi')
    .map(([, args]) => (args as { runtimeId: string }).runtimeId);
}

/** Open `count` sessions in order, leaving each one ready and idle. */
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

async function setupUnansweredPhantomCommand(): Promise<{
  tau: Tau;
  project: ProjectSummary;
  otherSession: SessionSummary;
  controller: SessionController;
}> {
  const { tau, project, secondSession } = await setupExtensionControllers();
  await tau.newSession(project);
  const controller = tau.state.controllers.find(
    (candidate) => candidate.phantom,
  );
  if (!controller) throw new Error('Expected a phantom controller');
  controller.starting = false;
  controller.ready = true;
  controller.commands = [{ name: 'mcp', source: 'extension' }];
  controller.commandsLoaded = true;
  controller.currentEffort = 'high';
  vi.mocked(invoke).mockClear();

  tau.draft.value = '/mcp';
  await tau.sendMessage();
  emitRpc(controller, {
    id: controller.pendingPrompt?.stateRequestId,
    type: 'response',
    command: 'get_state',
    success: true,
    data: {
      model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
      thinkingLevel: 'high',
      sessionId: 'unwritten-mcp',
      sessionFile: '/tmp/unwritten-mcp.jsonl',
      sessionName: '',
      isStreaming: false,
    },
  });
  await vi.waitFor(() => {
    expect(controller.pendingPrompt?.messagesRequestId).not.toBe('');
    expect(
      sentRequests(controller, 'get_messages').some(
        (request) => request.id === controller.pendingPrompt?.messagesRequestId,
      ),
    ).toBe(true);
  });
  emitRpc(controller, {
    id: controller.pendingPrompt?.messagesRequestId,
    type: 'response',
    command: 'get_messages',
    success: true,
    data: { messages: [] },
  });
  await vi.waitFor(() => {
    expect(controller.commandPromptRequestId).not.toBe('');
  });

  expect(controller.phantom).toBe(false);
  expect(controller.working).toBe(true);
  expect(
    vi
      .mocked(invoke)
      .mock.calls.some(([command]) => command === 'register_session'),
  ).toBe(false);
  return { tau, project, otherSession: secondSession, controller };
}

async function settleAndHydrateCompleted(
  controller: SessionController,
  assistantText: string,
): Promise<void> {
  const stateCount = sentRequests(controller, 'get_state').length;
  emitRpc(controller, {
    type: 'message_end',
    message: {
      role: 'assistant',
      content: [{ type: 'text', text: assistantText }],
    },
  });
  emitRpc(controller, { type: 'agent_settled' });
  await vi.waitFor(() => {
    expect(controller.materializationStateRequestId).not.toBe('');
    expect(sentRequests(controller, 'get_state').length).toBeGreaterThanOrEqual(
      stateCount + 2,
    );
  });
  emitRpc(controller, {
    id: controller.materializationStateRequestId,
    type: 'response',
    command: 'get_state',
    success: true,
    data: {
      model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
      thinkingLevel: 'high',
      sessionId: controller.sessionId,
      sessionFile: controller.sessionPath,
      sessionName: controller.sessionName,
      isStreaming: false,
    },
  });
  await vi.waitFor(() => {
    expect(controller.materializationMessagesRequestId).not.toBe('');
  });
  emitRpc(controller, {
    id: controller.materializationMessagesRequestId,
    type: 'response',
    command: 'get_messages',
    success: true,
    data: {
      messages: [
        { role: 'user', content: 'Prompt' },
        {
          role: 'assistant',
          content: [{ type: 'text', text: assistantText }],
        },
      ],
    },
  });
  await vi.waitFor(() => {
    expect(controller.materializationVerified).toBe(true);
  });
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
function savedSession(
  id: string,
  lastUserMessageAt = 0,
  sortAt = lastUserMessageAt,
): SessionSummary {
  return {
    id,
    path: `/tmp/${id}.jsonl`,
    title: id,
    lastActive: 'now',
    lastUserMessageAt,
    sortAt,
    archived: false,
    selected: false,
  };
}
