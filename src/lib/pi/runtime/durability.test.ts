import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
  type SessionSummary,
} from '../../../composables/state';
import type { PiBridgeEvent } from '../bridge';

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
describe('command-created session durability — durability', () => {
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

  it('hydrates bootstrap without waiting for registration persistence', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({ materializationVerified: true });
    addEphemeral(controller);
    state.workspace = registeredWorkspace(controller, 'Before');
    let finishRegistration!: (value: SessionSummary | null) => void;
    const registration = new Promise<SessionSummary | null>((resolve) => {
      finishRegistration = resolve;
    });
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session' ? registration : undefined,
    );
    const requestId = nextRequestId('bootstrap-state');
    controller.bootstrapStateRequestId = requestId;
    await rpc(controller, { id: requestId, type: 'get_state' });
    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: false,
      },
    });
    expect(controller.startMessagesRequestId).toBeTruthy();
    expect(mockInvoke).toHaveBeenCalledWith(
      'send_pi',
      expect.objectContaining({
        request: expect.objectContaining({ type: 'get_messages' }),
      }),
    );
    finishRegistration(
      registrationRow(registeredWorkspace(controller, 'After')),
    );
    await vi.waitFor(() => {
      expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe('After');
    });
  });

  it('keeps a newer archive when an older registration resolves', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({ materializationVerified: true });
    addEphemeral(controller);
    state.workspace = registeredWorkspace(controller, 'Before');
    let finishRegistration!: (value: SessionSummary | null) => void;
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? new Promise<SessionSummary | null>((resolve) => {
            finishRegistration = resolve;
          })
        : undefined,
    );
    const requestId = nextRequestId('bootstrap-state');
    controller.bootstrapStateRequestId = requestId;
    await rpc(controller, { id: requestId, type: 'get_state' });
    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
      },
    });
    await vi.waitFor(() => expect(finishRegistration).toBeTypeOf('function'));
    state.workspace.projects[0]!.sessions[0]!.archived = true;
    finishRegistration(
      registrationRow(registeredWorkspace(controller, 'After')),
    );
    await vi.waitFor(() => {
      expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe('After');
    });
    expect(state.workspace?.projects[0]?.sessions[0]?.archived).toBe(true);
  });

  it('does not register an identity reported by command sync alone', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    const commandSyncRequestId = nextRequestId('command-sync');
    controller.commandSyncRequestId = commandSyncRequestId;
    await rpc(controller, { id: commandSyncRequestId, type: 'get_state' });

    await handleResponse(controller, {
      id: commandSyncRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        sessionName: controller.sessionName,
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalled();
    expect(state.ephemeralSessions).toHaveLength(1);
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'keeps a user-only %s replacement ephemeral and discards it on release',
    async (_kind, connectionString) => {
      const telemetry = await import('../../telemetry');
      const { handleResponse, rpc, stopControllerProcess } =
        await import('./index');
      const controller = makeController({
        sessionId: 'user-only-session',
        sessionPath: '/tmp/project/user-only.jsonl',
        sessionName: 'User only',
        lastUserMessageAt: 42,
      });
      addEphemeral(controller, connectionString);
      const messagesRequestId = nextRequestId('messages');
      controller.materializationMessagesRequestId = messagesRequestId;
      await rpc(controller, {
        id: messagesRequestId,
        type: 'get_messages',
      });

      await handleResponse(controller, {
        id: messagesRequestId,
        command: 'get_messages',
        success: true,
        data: { messages: [{ role: 'user', content: 'Unanswered prompt' }] },
      });

      expect(controller.hasPiTranscript).toBe(true);
      expect(controller.materializationVerified).toBe(false);
      expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
        'register_session',
        expect.anything(),
        expect.anything(),
      );
      expect(state.ephemeralSessions).toHaveLength(1);

      await stopControllerProcess(controller, undefined, false);

      expect(state.ephemeralSessions).toEqual([]);
      expect(state.controllers).toEqual([]);
    },
  );

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'adopts an unverified %s session Pi did write when the process is lost',
    async (_kind, connectionString) => {
      const telemetry = await import('../../telemetry');
      const { handleBridgeEvent, handleRpc } = await import('./index');
      const controller = makeController({
        sessionId: 'materialized-session',
        sessionPath: '/tmp/project/materialized.jsonl',
        sessionName: 'Materialized',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      // A run this long never settles, so Tau holds no materialization proof.
      // Pi's file exists, so the probe's snapshot lists the session.
      const adopted = registeredWorkspace(controller, 'Materialized');
      vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
        command === 'register_session' ? registrationRow(adopted) : undefined,
      );

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Partial' },
      });
      await handleBridgeEvent({
        runtimeId: controller.runtimeId,
        generation: controller.generation,
        kind: 'exited',
        code: 1,
      } satisfies PiBridgeEvent);

      if (connectionString) {
        expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
          'register_session',
          expect.anything(),
          expect.anything(),
        );
        expect(state.ephemeralSessions).toHaveLength(1);
        expect(controller.remoteDisconnected).toBe(true);
      } else {
        expect(telemetry.invokeTraced).toHaveBeenCalledWith(
          'register_session',
          expect.objectContaining({
            sessionId: 'materialized-session',
            adopted: true,
          }),
          undefined,
        );
        expect(state.workspace).toEqual(adopted);
        expect(state.ephemeralSessions).toEqual([]);
      }
      expect(state.controllers).toHaveLength(1);
      expect(state.controllers[0]?.sessionId).toBe('materialized-session');
    },
  );

  it('keeps a session Pi swapped in while the stop was in flight', async () => {
    const telemetry = await import('../../telemetry');
    const { stopControllerProcess } = await import('./index');
    const controller = makeController({
      sessionId: 'predecessor-session',
      sessionPath: '/tmp/project/predecessor.jsonl',
      sessionName: 'Predecessor',
    });
    addEphemeral(controller);

    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) => {
      // Pi replaces the session on the live runtime, keeping the generation,
      // while the stop this controller authorised is still awaited.
      if (command === 'stop_pi') {
        controller.sessionId = 'successor-session';
        controller.sessionPath = '/tmp/project/successor.jsonl';
      }
      return undefined as never;
    });

    await stopControllerProcess(controller);

    expect(state.ephemeralSessions).toHaveLength(1);
    expect(state.controllers).toHaveLength(1);
    expect(state.controllers[0]?.sessionId).toBe('successor-session');
    expect(state.controllers[0]?.starting).toBe(true);
  });

  it('does not probe for replacement while a chained extension dialog is pending', async () => {
    const { handleRpc, probeSessionReplacement } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);

    await handleRpc(controller, {
      type: 'extension_ui_request',
      id: 'next-step',
      method: 'confirm',
      title: 'Continue?',
    });
    await probeSessionReplacement(controller, false);

    expect(controller.replacementProbeRequestId).toBe('');
    expect(
      mockInvoke.mock.calls.filter(
        ([command, args]) =>
          command === 'send_pi' &&
          (args as { request?: { type?: string } })?.request?.type ===
            'get_state',
      ),
    ).toEqual([]);
  });

  it('keeps an idle runtime until its final replacement probe answers', async () => {
    vi.useFakeTimers();
    try {
      const telemetry = await import('../../telemetry');
      const {
        canReleaseRuntime,
        handleResponse,
        releaseIdleRuntimes,
        watchingSessionReplacement,
        watchSessionReplacement,
      } = await import('./index');
      const controller = makeController({
        sessionId: 'settled-plan',
        sessionPath: '/tmp/project/settled-plan.jsonl',
      });
      state.controllers.push(
        controller,
        ...Array.from({ length: 6 }, (_, index) =>
          makeController({
            key: `controller-${index + 2}`,
            runtimeId: `runtime-${index + 2}`,
            sessionId: `idle-${index + 1}`,
            sessionPath: `/tmp/project/idle-${index + 1}.jsonl`,
            lastActiveSequence: index + 1,
          }),
        ),
      );

      watchSessionReplacement(controller);
      await vi.advanceTimersByTimeAsync(3_200);

      const requestId = controller.replacementProbeRequestId;
      expect(requestId).toMatch(/^tau-replacement-probe-/);
      expect(watchingSessionReplacement(controller)).toBe(true);
      expect(canReleaseRuntime(controller)).toBe(false);

      releaseIdleRuntimes();
      expect(
        vi
          .mocked(telemetry.invokeTraced)
          .mock.calls.some(([command]) => command === 'stop_pi'),
      ).toBe(false);

      await handleResponse(controller, {
        id: requestId,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          isStreaming: false,
        },
      });

      expect(watchingSessionReplacement(controller)).toBe(false);
      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'stop_pi',
        expect.objectContaining({ runtimeId: controller.runtimeId }),
        undefined,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'discards a partial assistant stream Pi never wrote after %s process loss',
    async (_kind, connectionString) => {
      const telemetry = await import('../../telemetry');
      const { handleBridgeEvent, handleRpc } = await import('./index');
      const controller = makeController({
        sessionId: 'partial-session',
        sessionPath: '/tmp/project/partial.jsonl',
        sessionName: 'Partial stream',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);

      vi.mocked(telemetry.invokeTraced).mockResolvedValue(null);

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Partial' },
      });
      await handleBridgeEvent({
        runtimeId: controller.runtimeId,
        generation: controller.generation,
        kind: 'exited',
        code: 1,
      } satisfies PiBridgeEvent);

      if (connectionString) {
        expect(state.ephemeralSessions).toHaveLength(1);
        expect(state.controllers).toEqual([controller]);
        expect(controller.remoteDisconnected).toBe(true);
      } else {
        expect(state.ephemeralSessions).toEqual([]);
        expect(state.controllers).toEqual([]);
      }
    },
  );

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'promotes a completed %s assistant after its message_end barrier while the run continues',
    async (_kind, connectionString) => {
      const telemetry = await import('../../telemetry');
      const { handleResponse, handleRpc } = await import('./index');
      const controller = makeController({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        sessionName: 'Completed work',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      const workspace = registeredWorkspace(controller, 'Completed work');
      let releaseRegistration: (() => void) | undefined;
      const registrationHeld = new Promise<void>((resolve) => {
        releaseRegistration = resolve;
      });
      vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) => {
        if (command === 'register_session') {
          await registrationHeld;
          return registrationRow(workspace);
        }
        return undefined;
      });

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'First finalized reply',
        },
      });
      expect(controller.materializationBarrierRequestId).toBe('');
      expect(telemetry.invokeTraced).not.toHaveBeenCalled();

      await handleRpc(controller, {
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'First finalized reply' }],
        },
      });
      const barrierRequestId = controller.materializationBarrierRequestId;
      expect(barrierRequestId).toMatch(/^tau-materialization-barrier-/);

      const promotion = handleResponse(controller, {
        id: barrierRequestId,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          sessionName: controller.sessionName,
          isStreaming: true,
        },
      });
      await vi.waitFor(() => {
        expect(telemetry.invokeTraced).toHaveBeenCalled();
      });
      expect(controller.materializationBarrierRequestId).toBe(barrierRequestId);
      releaseRegistration?.();
      await promotion;

      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({
          sessionId: controller.sessionId,
          adopted: true,
        }),
        undefined,
      );
      expect(controller.materializationVerified).toBe(true);
      expect(controller.streaming).toBe(true);
      expect(controller.working).toBe(true);
      expect(state.ephemeralSessions).toEqual([]);
      expect(
        mockInvoke.mock.calls.some(
          ([, args]) =>
            (args as { request?: { type?: string } })?.request?.type ===
            'get_messages',
        ),
      ).toBe(false);
    },
  );

  it('does not promote when the message_end barrier no longer owns the exact identity', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);

    await handleRpc(controller, {
      type: 'message_end',
      message: { role: 'assistant', content: [] },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;
    controller.sessionId = 'replacement-session';
    controller.sessionPath = '/tmp/project/replacement.jsonl';

    await handleResponse(controller, {
      id: barrierRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'completed-session',
        sessionFile: '/tmp/project/completed.jsonl',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalled();
    expect(controller.materializationVerified).toBe(false);
    expect(state.ephemeralSessions).toHaveLength(1);
  });

  it('registers the completed predecessor when its message_end barrier discovers a replacement', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      sessionName: 'Completed work',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? registrationRow(registeredWorkspace(controller, 'Completed work'))
        : undefined,
    );

    await handleRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [{ type: 'text', text: 'First finalized reply' }],
      },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;

    await handleResponse(controller, {
      id: barrierRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-session',
        sessionFile: '/tmp/project/replacement.jsonl',
        sessionName: 'Replacement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(
      state.workspace?.projects[0]?.sessions.some(
        (session) => session.id === 'completed-session',
      ),
    ).toBe(true);
    expect(controller.sessionId).toBe('replacement-session');
    expect(controller.materializationVerified).toBe(false);
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'replacement-session' }),
    ]);
  });

  it('keeps a completed predecessor proof when its successor starts before the barrier responds', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      sessionName: 'Completed work',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session'
        ? registrationRow(registeredWorkspace(controller, 'Completed work'))
        : undefined,
    );

    await handleRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [{ type: 'text', text: 'First finalized reply' }],
      },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;

    await handleRpc(controller, { type: 'agent_start' });
    expect(controller.materializationBarrierRequestId).toBe(barrierRequestId);
    const successorStateRequestId = controller.runStateRequestId;

    await handleResponse(controller, {
      id: successorStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-session',
        sessionFile: '/tmp/project/replacement.jsonl',
        sessionName: 'Replacement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(controller.sessionId).toBe('replacement-session');
    expect(controller.materializationBarrierRequestId).toBe('');
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'replacement-session' }),
    ]);
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'still promotes a completed %s assistant from settled hydration when its barrier was missed',
    async (_kind, connectionString) => {
      const telemetry = await import('../../telemetry');
      const { handleResponse, handleRpc } = await import('./index');
      const controller = makeController({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        sessionName: 'Completed work',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      const workspace = {
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
            sessions: [
              {
                id: controller.sessionId,
                path: controller.sessionPath,
                title: controller.sessionName,
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
      vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce(workspace);

      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Complete reply' },
      });
      expect(telemetry.invokeTraced).not.toHaveBeenCalled();

      await handleRpc(controller, { type: 'agent_settled' });
      const settledState = mockInvoke.mock.calls
        .map(
          ([, args]) =>
            (args as { request?: Record<string, unknown> })?.request,
        )
        .find((request) => request?.type === 'get_state');
      await handleResponse(controller, {
        id: settledState?.id,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          sessionName: controller.sessionName,
          isStreaming: false,
        },
      });
      const messagesRequestId = controller.materializationMessagesRequestId;
      expect(messagesRequestId).not.toBe('');

      await handleResponse(controller, {
        id: messagesRequestId,
        command: 'get_messages',
        success: true,
        data: {
          messages: [
            { role: 'user', content: 'Prompt' },
            {
              role: 'assistant',
              content: [{ type: 'text', text: 'Complete reply' }],
            },
          ],
        },
      });

      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({ sessionId: controller.sessionId }),
        undefined,
      );
      expect(controller.materializationVerified).toBe(true);
      expect(state.ephemeralSessions).toEqual([]);
    },
  );
});
