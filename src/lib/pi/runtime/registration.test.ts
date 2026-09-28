import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
  type SessionSummary,
} from '../../../composables/state';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock('../../telemetry', () => ({
  invokeTraced: vi.fn(async () => undefined),
  recordControllerTransition: vi.fn(),
  recordRpcResponseAnomaly: vi.fn(),
  recordStreamAggregate: vi.fn(),
  startRpcSpan: vi.fn(() => ({ context: undefined, end: vi.fn() })),
}));

const mockInvoke = vi.mocked(invoke);

async function dispatchRequest(
  controller: SessionController,
  type: string,
  prefix: string,
): Promise<string> {
  const { rpc } = await import('./index');
  const id = nextRequestId(prefix);
  await rpc(controller, { id, type });
  return id;
}

function registrationRow(
  workspace: NonNullable<typeof state.workspace>,
): SessionSummary | null {
  return workspace.projects[0]?.sessions[0] ?? null;
}

function makeController(
  overrides: Partial<SessionController> = {},
): SessionController {
  return sessionControllerFixture({
    sessionPath: '/tmp/project/session.jsonl',
    generation: 1,
    ready: true,
    ...overrides,
  });
}

beforeEach(async () => {
  mockInvoke.mockClear();
  state.controllers.splice(0);
  state.workspaceFeedback = [];
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.activeControllerKey = '';
  state.workspace = null;
  state.remoteRetry = undefined;
  state.remoteDialogOpen = false;
  state.remoteDialogMode = 'add';
  state.remoteConnectionError = '';
  state.remoteConnecting = false;
  const telemetry = await import('../../telemetry');
  vi.mocked(telemetry.invokeTraced).mockReset();
  vi.mocked(telemetry.invokeTraced).mockResolvedValue(undefined);
});
describe('command-created session durability — registration', () => {
  function addEphemeral(
    controller: SessionController,
    connectionString?: string,
  ): void {
    state.controllers.push(controller);
    state.ephemeralSessions.push({
      id: controller.sessionId,
      path: controller.sessionPath,
      title: controller.sessionName || 'Command session',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: true,
      projectPath: controller.projectPath,
      controllerKey: controller.key,
      createdAt: 1,
      phantom: false,
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          ...(connectionString ? { connectionString } : {}),
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
  }

  function registeredWorkspace(
    controller: SessionController,
    title: string,
    archived = false,
  ): NonNullable<typeof state.workspace> {
    return {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          collapsed: false,
          selected: true,
          sessions: [
            {
              id: controller.sessionId,
              path: controller.sessionPath,
              title,
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived,
              selected: false,
            },
          ],
        },
      ],
    };
  }

  it('merges only the registered row after unrelated workspace edits', async () => {
    const telemetry = await import('../../telemetry');
    const { registerConnectedSession } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Old title');
    let resolveRegistration!: (row: SessionSummary | null) => void;
    const registration = new Promise<SessionSummary | null>((resolve) => {
      resolveRegistration = resolve;
    });
    vi.mocked(telemetry.invokeTraced).mockReturnValueOnce(registration);

    const pending = registerConnectedSession(controller);
    await vi.waitFor(() =>
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1),
    );
    state.workspace.projects.push({
      path: '/other',
      name: 'Other',
      workingDirectory: '/other',
      collapsed: false,
      selected: true,
      sessions: [],
    });
    state.workspace.projects[0]!.collapsed = true;
    state.workspace.activeProjectPath = '/other';
    resolveRegistration({
      ...registrationRow(registeredWorkspace(controller, 'New title'))!,
      title: 'New title',
    });
    await pending;

    expect(state.workspace.projects.map((project) => project.path)).toEqual([
      controller.projectPath,
      '/other',
    ]);
    expect(state.workspace.projects[0]).toMatchObject({ collapsed: true });
    expect(state.workspace.activeProjectPath).toBe('/other');
    expect(state.workspace.projects[0]?.sessions[0]?.title).toBe('New title');
  });

  it('does not rewrite selection when registration reports it already selected', async () => {
    const telemetry = await import('../../telemetry');
    const { registerConnectedSession } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Old title');
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeControllerKey = controller.key;
    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
      ...registrationRow(registeredWorkspace(controller, 'New title'))!,
      selected: true,
    });

    await registerConnectedSession(controller);

    expect(state.workspace.projects[0]?.sessions[0]?.title).toBe('New title');
    expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1);
  });

  it('keeps an unmaterialized registration null without replacing the workspace', async () => {
    const telemetry = await import('../../telemetry');
    const { registerConnectedSession } = await import('./index');
    const controller = makeController({ materializationVerified: true });
    addEphemeral(controller);
    const workspace = state.workspace;
    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce(null);

    await registerConnectedSession(controller);

    expect(state.workspace).toBe(workspace);
    expect(state.ephemeralSessions[0]?.id).toBe(controller.sessionId);
    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'set_active_session',
      expect.anything(),
      undefined,
    );
  });

  it('retiring an unsaved registered row applies only the archive mutation', async () => {
    const telemetry = await import('../../telemetry');
    const { retireUnsavedSession } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Stale');
    const unrelated = {
      ...state.workspace.projects[0]!.sessions[0]!,
      id: 'other',
      title: 'Other',
    };
    state.workspace.projects[0]!.sessions.push(unrelated);
    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
      sessionId: 'session-1',
      archived: true,
      activeSessionId: 'other',
    });

    await retireUnsavedSession(controller);

    expect(state.workspace.projects[0]?.sessions).toMatchObject([
      { id: 'session-1', archived: true, selected: false },
      { id: 'other', title: 'Other', selected: true },
    ]);
    expect(controller.phantom).toBe(true);
    expect(telemetry.invokeTraced).toHaveBeenCalledWith('archive_session', {
      projectPath: controller.projectPath,
      sessionId: 'session-1',
    });
  });

  it('projects only the latest title from overlapping registrations', async () => {
    const telemetry = await import('../../telemetry');
    const { persistSessionName } = await import('./index');
    const controller = makeController({
      sessionId: 'title-session',
      sessionPath: '/tmp/project/title-session.jsonl',
      sessionName: 'First title',
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Original title');

    let resolveFirst!: (workspace: NonNullable<typeof state.workspace>) => void;
    let resolveSecond!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const firstRegistration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveFirst = resolve;
      },
    );
    const secondRegistration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveSecond = resolve;
      },
    );
    vi.mocked(telemetry.invokeTraced)
      .mockReturnValueOnce(firstRegistration.then(registrationRow))
      .mockReturnValueOnce(secondRegistration.then(registrationRow));

    const first = persistSessionName(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1);
    });
    controller.sessionName = 'Second title';
    const second = persistSessionName(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(2);
    });

    resolveSecond(registeredWorkspace(controller, 'Second title'));
    await second;
    expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe(
      'Second title',
    );
    resolveFirst(registeredWorkspace(controller, 'First title'));
    await first;
    expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe(
      'Second title',
    );
    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.map(
          ([, args]) => (args as { sessionName?: string }).sessionName,
        ),
    ).toEqual(['First title', 'Second title']);
  });

  it('promotes a materialized external transcript but not an absent row', async () => {
    const telemetry = await import('../../telemetry');
    const { registerConnectedSession } = await import('./index');
    const { canArchiveSession } = await import('../../../composables/state');
    const controller = makeController({
      sessionId: 'external-session',
      sessionPath: '/tmp/mission/sessions/external-session.jsonl',
      sessionName: 'Mission',
      materializationVerified: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce(null);
    await registerConnectedSession(controller);
    expect(state.ephemeralSessions).toHaveLength(1);
    expect(
      canArchiveSession(
        state.workspace!.projects[0]!,
        state.ephemeralSessions[0]!,
      ),
    ).toBe(false);

    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce(
      registrationRow(registeredWorkspace(controller, 'Mission')),
    );
    await registerConnectedSession(controller);
    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        projectPath: controller.projectPath,
        sessionPath: '/tmp/mission/sessions/external-session.jsonl',
      }),
      undefined,
    );
    expect(state.ephemeralSessions).toHaveLength(0);
    expect(
      canArchiveSession(
        state.workspace!.projects[0]!,
        state.workspace!.projects[0]!.sessions[0]!,
      ),
    ).toBe(true);
  });

  it('does not let a weaker registration overwrite overlapping adoption', async () => {
    const telemetry = await import('../../telemetry');
    const { registerConnectedSession } = await import('./index');
    const controller = makeController({
      sessionId: 'adoption-session',
      sessionPath: '/tmp/project/adoption-session.jsonl',
      sessionName: 'Session',
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Session', true);

    let resolveOrdinary!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    let resolveAdoption!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const ordinaryRegistration = new Promise<
      NonNullable<typeof state.workspace>
    >((resolve) => {
      resolveOrdinary = resolve;
    });
    const adoptionRegistration = new Promise<
      NonNullable<typeof state.workspace>
    >((resolve) => {
      resolveAdoption = resolve;
    });
    vi.mocked(telemetry.invokeTraced)
      .mockReturnValueOnce(ordinaryRegistration.then(registrationRow))
      .mockReturnValueOnce(adoptionRegistration.then(registrationRow));

    const ordinary = registerConnectedSession(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1);
    });
    const adoption = registerConnectedSession(controller, undefined, true);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(2);
    });

    resolveAdoption(registeredWorkspace(controller, 'Session'));
    await adoption;
    resolveOrdinary(registeredWorkspace(controller, 'Session', true));
    await ordinary;

    expect(state.workspace?.projects[0]?.sessions[0]?.archived).toBe(false);
    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.map(([, args]) => (args as { adopted?: boolean }).adopted),
    ).toEqual([false, true]);
  });

  it('does not register a replacement that races session selection', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    let resolveSelection!: (workspace: typeof registeredWorkspace) => void;
    const selection = new Promise<typeof registeredWorkspace>((resolve) => {
      resolveSelection = resolve;
    });
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'set_active_session'
        ? await selection.then(() => undefined)
        : registrationRow(registeredWorkspace),
    );
    const verificationRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = verificationRequestId;
    await rpc(controller, {
      id: verificationRequestId,
      type: 'get_messages',
    });

    const promotion = handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'assistant', content: 'Durable A' }] },
    });
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'set_active_session',
        expect.objectContaining({ sessionId: 'verified-a' }),
        undefined,
      );
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        isStreaming: true,
      },
    });
    resolveSelection(registeredWorkspace);
    await promotion;

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['verified-a']);
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });

  it('does not adopt a replacement that races selection fallback', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    let resolveAdoption!: (workspace: typeof registeredWorkspace) => void;
    const adoption = new Promise<typeof registeredWorkspace>((resolve) => {
      resolveAdoption = resolve;
    });
    let registrationCount = 0;
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) => {
      if (command === 'set_active_session') {
        throw new Error('Missing registry row');
      }
      if (command === 'register_session') {
        registrationCount += 1;
        if (registrationCount === 2) return registrationRow(await adoption);
        return registrationRow(registeredWorkspace);
      }
      return undefined;
    });
    const verificationRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = verificationRequestId;
    await rpc(controller, {
      id: verificationRequestId,
      type: 'get_messages',
    });

    const promotion = handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'assistant', content: 'Durable A' }] },
    });
    await vi.waitFor(() => {
      expect(registrationCount).toBe(2);
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        isStreaming: true,
      },
    });
    resolveAdoption(registeredWorkspace);
    await promotion;

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => ({
          sessionId: (args as { sessionId: string }).sessionId,
          adopted: (args as { adopted?: boolean }).adopted,
        })),
    ).toEqual([
      { sessionId: 'verified-a', adopted: true },
      { sessionId: 'verified-a', adopted: true },
    ]);
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });
});
