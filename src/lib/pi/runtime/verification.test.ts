import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
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
describe('command-created session durability — verification', () => {
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

  it('retries verification after a failed settled message hydration', async () => {
    const telemetry = await import('../../telemetry');
    const { handleResponse, probeSessionReplacement, rpc } =
      await import('./index');
    const controller = makeController({
      sessionId: 'retry-session',
      sessionPath: '/tmp/project/retry.jsonl',
      postSettlementHydration: true,
      syncing: true,
    });
    addEphemeral(controller);
    const failedMessagesRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = failedMessagesRequestId;
    await rpc(controller, {
      id: failedMessagesRequestId,
      type: 'get_messages',
    });

    await handleResponse(controller, {
      id: failedMessagesRequestId,
      command: 'get_messages',
      success: false,
    });

    expect(controller.materializationMessagesRequestId).toBe('');
    expect(controller.postSettlementHydration).toBe(true);
    expect(controller.syncing).toBe(false);

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: false,
      },
    });
    const retryRequestId = controller.materializationMessagesRequestId;
    expect(retryRequestId).not.toBe('');

    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
      ...state.workspace,
      projects: [
        {
          ...state.workspace!.projects[0],
          sessions: [
            {
              id: controller.sessionId,
              path: controller.sessionPath,
              title: 'Retry session',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    });
    await handleResponse(controller, {
      id: retryRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Retry succeeded' }],
          },
        ],
      },
    });

    expect(controller.materializationVerified).toBe(true);
    expect(controller.postSettlementHydration).toBe(false);
    expect(state.ephemeralSessions).toEqual([]);
  });

  it.each([
    ['effort response', 'get_available_thinking_levels', 'response'],
    ['message response', 'get_messages', 'response'],
    ['effort transport', 'get_available_thinking_levels', 'transport'],
    ['message transport', 'get_messages', 'transport'],
  ] as const)(
    'registers replacement B after a failed %s via the bounded verification timer',
    async (_label, failedMethod, failure) => {
      vi.useFakeTimers();
      try {
        const telemetry = await import('../../telemetry');
        const {
          handleResponse,
          handleRpc,
          watchingMaterializationVerification,
          watchingSessionReplacement,
        } = await import('./index');
        const controller = makeController({
          sessionId: 'identity-a',
          sessionPath: '/tmp/project/identity-a.jsonl',
        });
        addEphemeral(controller);

        let failedTransport = false;
        if (failure === 'transport') {
          mockInvoke.mockImplementation(async (command, args) => {
            const request = (args as { request?: Record<string, unknown> })
              ?.request;
            if (
              command === 'send_pi' &&
              request?.type === failedMethod &&
              controller.sessionId === 'identity-b' &&
              !failedTransport
            ) {
              failedTransport = true;
              throw new Error('transport unavailable');
            }
            return undefined;
          });
        }

        await handleRpc(controller, { type: 'agent_settled' });
        const discoveringStateRequestId =
          controller.materializationStateRequestId;
        const discovery = handleResponse(controller, {
          id: discoveringStateRequestId,
          command: 'get_state',
          success: true,
          data: {
            sessionId: 'identity-b',
            sessionFile: '/tmp/project/identity-b.jsonl',
            isStreaming: false,
          },
        });
        if (failure === 'transport') await expect(discovery).rejects.toThrow();
        else await discovery;

        expect(watchingSessionReplacement(controller)).toBe(false);
        if (failure === 'response') {
          const failedRequest = mockInvoke.mock.calls
            .filter(([command]) => command === 'send_pi')
            .map(
              ([, args]) =>
                (args as { request: Record<string, unknown> }).request,
            )
            .reverse()
            .find((request) => request.type === failedMethod);
          await handleResponse(controller, {
            id: failedRequest?.id,
            command: failedMethod,
            success: false,
          });
        }
        expect(watchingMaterializationVerification(controller)).toBe(true);

        await vi.advanceTimersByTimeAsync(250);
        const retryStateRequestId = controller.materializationStateRequestId;
        expect(retryStateRequestId).toMatch(/^tau-materialization-retry-/);
        await handleResponse(controller, {
          id: retryStateRequestId,
          command: 'get_state',
          success: true,
          data: {
            sessionId: 'identity-b',
            sessionFile: '/tmp/project/identity-b.jsonl',
            sessionName: 'Replacement B',
            isStreaming: false,
          },
        });
        const retryMessagesRequestId =
          controller.materializationMessagesRequestId;

        vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
          ...state.workspace!,
          projects: [
            {
              ...state.workspace!.projects[0],
              sessions: [
                {
                  id: 'identity-b',
                  path: '/tmp/project/identity-b.jsonl',
                  title: 'Replacement B',
                  lastActive: 'now',
                  lastUserMessageAt: 0,
                  sortAt: 1,
                  archived: false,
                  selected: false,
                },
              ],
            },
          ],
        });
        await handleResponse(controller, {
          id: retryMessagesRequestId,
          command: 'get_messages',
          success: true,
          data: {
            messages: [
              {
                role: 'assistant',
                content: [{ type: 'text', text: 'B completed' }],
              },
            ],
          },
        });

        expect(controller.materializationVerified).toBe(true);
        expect(watchingMaterializationVerification(controller)).toBe(false);
        expect(state.ephemeralSessions).toEqual([]);
        expect(
          vi
            .mocked(telemetry.invokeTraced)
            .mock.calls.some(
              ([command, args]) =>
                command === 'register_session' &&
                (args as { sessionId: string }).sessionId === 'identity-b',
            ),
        ).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    },
  );

  it('stops retrying and releases an unverified identity after the bounded policy', async () => {
    vi.useFakeTimers();
    try {
      const { handleResponse, rpc, watchingMaterializationVerification } =
        await import('./index');
      const controller = makeController({
        postSettlementHydration: true,
        syncing: true,
      });
      addEphemeral(controller);
      const failedStateRequestId = nextRequestId('settled-state');
      controller.materializationStateRequestId = failedStateRequestId;
      await rpc(controller, { id: failedStateRequestId, type: 'get_state' });

      mockInvoke.mockImplementation(async (command, args) => {
        const request = (args as { request?: Record<string, unknown> })
          ?.request;
        if (
          command === 'send_pi' &&
          request?.type === 'get_state' &&
          String(request.id).startsWith('tau-materialization-retry-')
        ) {
          throw new Error('transport unavailable');
        }
        return undefined;
      });
      await handleResponse(controller, {
        id: failedStateRequestId,
        command: 'get_state',
        success: false,
      });

      expect(watchingMaterializationVerification(controller)).toBe(true);
      await vi.advanceTimersByTimeAsync(250);
      await vi.advanceTimersByTimeAsync(750);
      await vi.advanceTimersByTimeAsync(1_500);
      await vi.runAllTimersAsync();

      expect(watchingMaterializationVerification(controller)).toBe(false);
      expect(controller.postSettlementHydration).toBe(false);
      expect(controller.generation).toBe(0);
      expect(state.ephemeralSessions).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels a pending materialization retry when the controller is released', async () => {
    vi.useFakeTimers();
    try {
      const {
        handleResponse,
        rpc,
        stopControllerProcess,
        watchingMaterializationVerification,
      } = await import('./index');
      const controller = makeController({
        postSettlementHydration: true,
        syncing: true,
      });
      addEphemeral(controller);
      const failedRequestId = nextRequestId('settled-state');
      controller.materializationStateRequestId = failedRequestId;
      await rpc(controller, { id: failedRequestId, type: 'get_state' });
      await handleResponse(controller, {
        id: failedRequestId,
        command: 'get_state',
        success: false,
      });
      expect(watchingMaterializationVerification(controller)).toBe(true);

      const sendsBeforeRelease = mockInvoke.mock.calls.filter(
        ([command]) => command === 'send_pi',
      ).length;
      await stopControllerProcess(controller, undefined, false);
      await vi.runAllTimersAsync();

      expect(watchingMaterializationVerification(controller)).toBe(false);
      expect(
        mockInvoke.mock.calls.filter(([command]) => command === 'send_pi'),
      ).toHaveLength(sendsBeforeRelease);
    } finally {
      vi.useRealTimers();
    }
  });

  it('ignores a predecessor state failure while verifying its replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./index');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    const predecessorRequestId = controller.materializationStateRequestId;
    expect(predecessorRequestId).toMatch(/^tau-settled-state-/);

    await probeSessionReplacement(controller, false);
    const replacementRequestId = controller.replacementProbeRequestId;
    expect(replacementRequestId).toMatch(/^tau-replacement-probe-/);

    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    expect(verificationRequestId).toMatch(/^tau-messages-/);
    expect(controller.generation).toBe(1);
    expect(controller.syncing).toBe(true);

    await handleResponse(controller, {
      id: predecessorRequestId,
      command: 'get_state',
      success: false,
    });

    expect(controller.disposed).toBe(false);
    expect(controller.generation).toBe(1);
    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.syncing).toBe(true);
    expect(controller.materializationStateRequestId).toBe('');
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({
        id: 'identity-b',
        path: '/tmp/project/identity-b.jsonl',
      }),
    ]);
  });

  it('ignores a predecessor state success while hydrating its replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./index');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    const predecessorRequestId = controller.materializationStateRequestId;
    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    controller.messages = [
      { id: 'identity-b-message', kind: 'assistant', text: 'B transcript' },
    ];

    await handleResponse(controller, {
      id: predecessorRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        sessionName: 'Identity A',
        isStreaming: false,
        thinkingLevel: 'off',
      },
    });

    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('ignores predecessor message and effort successes after replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./index');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    await handleResponse(controller, {
      id: controller.materializationStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        isStreaming: false,
      },
    });
    const predecessorMessagesRequestId =
      controller.materializationMessagesRequestId;
    const outboundRequests = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      );
    const predecessorEffortRequestId = String(
      [...outboundRequests]
        .reverse()
        .find((request) => request.type === 'get_available_thinking_levels')
        ?.id,
    );

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    const replacementEffortRequest = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .reverse()
      .find((request) => request.type === 'get_available_thinking_levels');
    await handleResponse(controller, {
      id: replacementEffortRequest?.id,
      command: 'get_available_thinking_levels',
      success: true,
      data: { levels: ['high', 'max'] },
    });
    controller.messages = [
      { id: 'identity-b-message', kind: 'assistant', text: 'B transcript' },
    ];

    await handleResponse(controller, {
      id: predecessorMessagesRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'assistant', content: 'Stale A transcript' }],
      },
    });

    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );

    await handleResponse(controller, {
      id: predecessorEffortRequestId,
      command: 'get_available_thinking_levels',
      success: true,
      data: { levels: ['off'] },
    });

    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.efforts).toEqual(['high', 'max']);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('ignores predecessor message and effort failures after replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./index');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    await handleResponse(controller, {
      id: controller.materializationStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        isStreaming: false,
      },
    });
    const predecessorMessagesRequestId =
      controller.materializationMessagesRequestId;
    const sentRequests = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      );
    const predecessorEffortRequestId = String(
      [...sentRequests]
        .reverse()
        .find((request) => request.type === 'get_available_thinking_levels')
        ?.id,
    );
    expect(predecessorMessagesRequestId).toMatch(/^tau-messages-/);
    expect(predecessorEffortRequestId).toMatch(/^tau-efforts-/);

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    expect(verificationRequestId).toMatch(/^tau-messages-/);
    expect(verificationRequestId).not.toBe(predecessorMessagesRequestId);

    await handleResponse(controller, {
      id: predecessorMessagesRequestId,
      command: 'get_messages',
      success: false,
    });
    await handleResponse(controller, {
      id: predecessorEffortRequestId,
      command: 'get_available_thinking_levels',
      success: false,
    });

    expect(controller.disposed).toBe(false);
    expect(controller.generation).toBe(1);
    expect(controller.sessionId).toBe('identity-b');
    expect(controller.syncing).toBe(true);
    expect(feedbackMessage(controller)).toBe('');
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('removes a command-only session when its runtime is released', async () => {
    const { stopControllerProcess } = await import('./index');
    const controller = makeController({
      generation: 1,
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
    });
    addEphemeral(controller);

    await stopControllerProcess(controller, undefined, false);

    expect(state.ephemeralSessions).toEqual([]);
    expect(state.controllers).toEqual([]);
  });

  it('keeps an empty workflow successor while materialization is pending', async () => {
    const telemetry = await import('../../telemetry');
    const { removeEmptyActivePhantom } = await import('./index');
    const controller = makeController({
      sessionId: 'implementation-session',
      sessionPath: '/tmp/project/implementation-session.jsonl',
      sessionName: 'Implement',
      postSettlementHydration: true,
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    removeEmptyActivePhantom();

    expect(state.ephemeralSessions).toHaveLength(1);
    expect(state.controllers).toEqual([controller]);
    expect(controller.disposed).toBe(false);
    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.some(([command]) => command === 'stop_pi'),
    ).toBe(false);
  });

  it('removes a command-only notification session when the user leaves', async () => {
    const { removeEmptyActivePhantom } = await import('./index');
    const controller = makeController({
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
      messages: [{ id: 'notice', kind: 'notice', text: 'Usage: 10%' }],
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    removeEmptyActivePhantom();

    expect(state.ephemeralSessions).toEqual([]);
    expect(state.controllers).toEqual([]);
    expect(state.activeSessionId).toBe('');
  });
});
