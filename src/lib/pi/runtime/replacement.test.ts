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

function feedbackMessage(controller: SessionController): string {
  return (
    [...controller.feedback]
      .reverse()
      .find((incident) => !incident.acknowledged)?.message ?? ''
  );
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
describe('command-created session durability — replacement', () => {
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

  it('registers a completed predecessor before settlement verification applies its replacement', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      syncing: true,
      messages: [
        { id: 'plan-reply', kind: 'assistant', text: 'Completed plan' },
      ],
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
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockImplementation(
      async (command, args) => {
        if (command === 'register_session') {
          expect(controller.sessionId).toBe('plan-a');
          expect(state.ephemeralSessions[0]).toMatchObject({
            id: 'plan-a',
            path: '/tmp/project/plan-a.jsonl',
            title: 'Plan',
          });
          expect(args).toMatchObject({
            sessionId: 'plan-a',
            sessionPath: '/tmp/project/plan-a.jsonl',
            sessionName: 'Plan',
            adopted: true,
          });
        }
        return command === 'register_session'
          ? registrationRow(registeredWorkspace)
          : undefined;
      },
    );
    const verificationRequestId = nextRequestId('settled-state');
    controller.materializationStateRequestId = verificationRequestId;
    await rpc(controller, { id: verificationRequestId, type: 'get_state' });

    await handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['plan-a']);
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'implement-b',
      path: '/tmp/project/implement-b.jsonl',
      title: 'Implement',
    });
    expect(controller).toMatchObject({
      sessionId: 'implement-b',
      sessionPath: '/tmp/project/implement-b.jsonl',
      sessionName: 'Implement',
      materializationVerified: false,
    });
  });

  it('preserves a settled predecessor when successor start reveals its replacement', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      messages: [{ id: 'plan-user', kind: 'user', text: 'Write the plan' }],
    });
    addEphemeral(controller);
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? registrationRow(registeredWorkspace)
        : undefined,
    );

    await handleRpc(controller, { type: 'agent_start' });
    const runStateRequestId = controller.runStateRequestId;
    await handleResponse(controller, {
      id: runStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({ sessionId: 'plan-a', adopted: true }),
      undefined,
    );
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]?.id).toBe('implement-b');
  });

  it('clears settled predecessor evidence when successor start keeps the identity', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      postSettlementHydration: true,
      settledAssistantActivity: true,
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_start' });
    expect(controller.postSettlementHydration).toBe(true);
    expect(controller.settledAssistantActivity).toBe(true);
    await handleResponse(controller, {
      id: controller.runStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'plan-a',
        sessionFile: '/tmp/project/plan-a.jsonl',
        isStreaming: true,
      },
    });

    expect(controller.postSettlementHydration).toBe(false);
    expect(controller.settledAssistantActivity).toBe(false);
  });

  it('preserves a settled predecessor when a later probe discovers its replacement', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      messages: [{ id: 'plan-user', kind: 'user', text: 'Write the plan' }],
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
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? registrationRow(registeredWorkspace)
        : undefined,
    );
    const probeRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = probeRequestId;

    await handleResponse(controller, {
      id: probeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'plan-a',
        sessionPath: '/tmp/project/plan-a.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'implement-b',
      path: '/tmp/project/implement-b.jsonl',
      title: 'Implement',
      lastActive: 'now',
    });
    expect(state.ephemeralSessions[0]!.sortAt).toBeGreaterThan(2);
    expect(state.activeSessionId).toBe('implement-b');
    expect(state.activeSessionPath).toBe('/tmp/project/implement-b.jsonl');
  });

  it('does not apply a successor name notification to its registered predecessor', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      materializationVerified: true,
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Plan');
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    await handleRpc(controller, {
      type: 'session_info_changed',
      name: 'Implement',
    });

    expect(controller.sessionName).toBe('Plan');
    expect(state.workspace.projects[0]?.sessions).toEqual([
      expect.objectContaining({ id: 'plan-a', title: 'Plan' }),
    ]);
    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'register_session',
      expect.anything(),
      expect.anything(),
    );

    await handleResponse(controller, {
      id: controller.sessionNameStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(state.workspace.projects[0]?.sessions).toEqual([
      expect.objectContaining({ id: 'plan-a', title: 'Plan' }),
    ]);
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'implement-b', title: 'Implement' }),
    ]);
  });

  it('does not carry an outgoing optimistic rename into its successor', async () => {
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Optimistic Plan',
      materializationVerified: true,
      pendingSessionRename: {
        requestId: 'rename-plan',
        previousName: 'Plan',
        previousTitle: 'Plan',
      },
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Optimistic Plan');
    await rpc(controller, {
      id: 'rename-plan',
      type: 'set_session_name',
      name: 'Optimistic Plan',
    });
    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = replacementRequestId;

    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });
    expect(controller.pendingSessionRename).toBeUndefined();
    expect(controller.sessionName).toBe('Implement');

    await handleResponse(controller, {
      id: 'rename-plan',
      command: 'set_session_name',
      success: false,
    });
    expect(controller.sessionName).toBe('Implement');
    expect(controller.feedback).toEqual([]);
  });

  it('keeps name refresh failures and timeouts cosmetic and retryable', async () => {
    vi.useFakeTimers();
    try {
      const { handleResponse, handleRpc } = await import('./index');
      const controller = makeController({ sessionName: 'Safe name' });
      state.controllers.push(controller);
      state.workspace = registeredWorkspace(controller, 'Safe name');

      await handleRpc(controller, {
        type: 'session_info_changed',
        name: 'Untrusted failure payload',
      });
      const failedRequestId = controller.sessionNameStateRequestId;
      await handleResponse(controller, {
        id: failedRequestId,
        command: 'get_state',
        success: false,
      });

      expect(controller.sessionNameStateRequestId).toBe('');
      expect(controller.sessionName).toBe('Safe name');
      expect(controller.ready).toBe(true);
      expect(controller.feedback).toEqual([]);

      await handleRpc(controller, {
        type: 'session_info_changed',
        name: 'Untrusted timeout payload',
      });
      const timedOutRequestId = controller.sessionNameStateRequestId;
      expect(timedOutRequestId).not.toBe('');
      await vi.advanceTimersByTimeAsync(10_000);
      expect(controller.sessionNameStateRequestId).toBe('');
      expect(controller.ready).toBe(true);

      await handleResponse(controller, {
        id: timedOutRequestId,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          sessionName: 'Late unsafe name',
          isStreaming: false,
        },
      });
      expect(controller.sessionName).toBe('Safe name');
    } finally {
      vi.useRealTimers();
    }
  });

  it('keeps a registered predecessor title when a probe discovers its successor', async () => {
    const { handleResponse } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      materializationVerified: true,
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Plan');
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const probeRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = probeRequestId;

    await handleResponse(controller, {
      id: probeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(state.workspace?.projects[0]?.sessions).toEqual([
      expect.objectContaining({ id: 'plan-a', title: 'Plan' }),
    ]);
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'implement-b', title: 'Implement' }),
    ]);
  });

  it('keeps the predecessor reachable when registration fails', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse } = await import('./index');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      syncing: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockRejectedValue(
      new Error('registration failed'),
    );
    const requestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = requestId;

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(controller.sessionId).toBe('plan-a');
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'plan-a',
      path: '/tmp/project/plan-a.jsonl',
      title: 'Plan',
    });
    expect(feedbackMessage(controller)).toBe(
      'This session could not be saved. Continue here, then try reopening it.',
    );
    expect(controller.syncing).toBe(false);
  });

  it('does not register an empty predecessor discovered by settlement verification', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'empty-a',
      sessionPath: '/tmp/project/empty-a.jsonl',
      sessionName: 'Empty',
      postSettlementHydration: true,
    });
    addEphemeral(controller);
    const requestId = nextRequestId('settled-state');
    controller.materializationStateRequestId = requestId;
    await rpc(controller, { id: requestId, type: 'get_state' });

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-b',
        sessionFile: '/tmp/project/replacement-b.jsonl',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'register_session',
      expect.anything(),
      expect.anything(),
    );
  });

  it('does not register a partial predecessor outside settlement verification', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse } = await import('./index');
    const controller = makeController({
      sessionId: 'partial-a',
      sessionPath: '/tmp/project/partial-a.jsonl',
      sessionName: 'Partial',
      messages: [{ id: 'partial', kind: 'assistant', text: 'Partial reply' }],
    });
    addEphemeral(controller);
    const requestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = requestId;

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-b',
        sessionFile: '/tmp/project/replacement-b.jsonl',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'register_session',
      expect.anything(),
      expect.anything(),
    );
  });

  it('commits a predecessor registration already in flight when its replacement arrives', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
      sessionName: 'Verified A',
      postSettlementHydration: true,
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    let resolveRegistration!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const registration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveRegistration = resolve;
      },
    );
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? registrationRow(await registration)
        : undefined,
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
      data: {
        messages: [
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'A is durable' }],
          },
        ],
      },
    });
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({ sessionId: 'verified-a' }),
        undefined,
      );
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = replacementRequestId;
    const replacement = handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        sessionName: 'Unverified B',
        isStreaming: true,
      },
    });
    await Promise.resolve();
    expect(controller.sessionId).toBe('verified-a');

    resolveRegistration({
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
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    });
    await Promise.all([promotion, replacement]);

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['verified-a']);
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'verified-a' })]),
    );
    expect(state.activeSessionId).toBe('unverified-b');
    expect(state.activeSessionPath).toBe('/tmp/project/unverified-b.jsonl');
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });
});
