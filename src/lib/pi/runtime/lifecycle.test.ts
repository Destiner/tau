import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import { state, type SessionController } from '../../../composables/state';

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
    compactionReason: undefined,
    compactionStartedWhileStreaming: false,
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

describe('runtime failures and recovery', () => {
  it.each(['agent_start', 'agent_settled', 'generation', 'exit', 'stop'])(
    'clears compaction evidence at the %s boundary',
    async (boundary) => {
      const { handleRpc, handleBridgeEvent, stopControllerProcess } =
        await import('./index');
      const controller = makeController({
        compacting: true,
        compactionReason: 'overflow',
        compactionStartedWhileStreaming: true,
        compactionReconciliationPending: true,
        compactionStreamSequence: 7,
      });
      state.controllers.push(controller);
      if (boundary === 'generation' || boundary === 'exit') {
        await handleBridgeEvent({
          runtimeId: controller.runtimeId,
          generation: boundary === 'generation' ? 2 : 1,
          kind: boundary === 'generation' ? 'started' : 'exited',
        });
      } else if (boundary === 'stop') {
        await stopControllerProcess(controller);
      } else {
        await handleRpc(controller, { type: boundary });
      }
      expect(controller.compactionReason).toBeUndefined();
      expect(controller.compactionStartedWhileStreaming).toBe(false);
      if (boundary === 'agent_start' || boundary === 'agent_settled') {
        expect(controller.compactionReconciliationPending).toBe(true);
        expect(controller.compactionStreamSequence).toBe(7);
      }
    },
  );

  it('ignores compaction evidence from an abandoned generation', async () => {
    const { handleBridgeEvent } = await import('./index');
    const controller = makeController({ generation: 2 });
    state.controllers.push(controller);
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: 1,
      kind: 'rpc',
      line: JSON.stringify({ type: 'compaction_start', reason: 'threshold' }),
    });
    expect(controller.compacting).toBe(false);
    expect(controller.compactionReason).toBeUndefined();
    expect(controller.compactionStartedWhileStreaming).toBe(false);
  });

  it('does not evict command work or failed refreshes', async () => {
    const { canReleaseRuntime } = await import('./index');
    const controller = makeController({ key: 'idle-command' });
    expect(canReleaseRuntime(controller)).toBe(true);
    controller.commandPromptRequestId = 'command';
    expect(canReleaseRuntime(controller)).toBe(false);
    controller.commandPromptRequestId = '';
    controller.commandSyncRequestId = 'state';
    expect(canReleaseRuntime(controller)).toBe(false);
    controller.commandSyncRequestId = '';
    controller.commandMessagesRequestId = 'messages';
    expect(canReleaseRuntime(controller)).toBe(false);
    controller.commandMessagesRequestId = '';
    controller.commandRefreshFailed = true;
    expect(canReleaseRuntime(controller)).toBe(false);
    controller.commandRefreshFailed = false;
    expect(canReleaseRuntime(controller)).toBe(true);
  });
  it('evicts truly idle controllers before a pending command under cache pressure', async () => {
    const { releaseIdleRuntimes } = await import('./index');
    const pending = makeController({
      key: 'pending-command',
      runtimeId: 'pending-runtime',
      commandPromptRequestId: 'command',
      lastActiveSequence: 0,
    });
    state.controllers.push(pending);
    for (let index = 1; index <= 8; index += 1) {
      state.controllers.push(
        makeController({
          key: `idle-${index}`,
          runtimeId: `idle-runtime-${index}`,
          lastActiveSequence: index,
        }),
      );
    }
    const { invokeTraced } = await import('../../telemetry');
    releaseIdleRuntimes();
    expect(
      vi
        .mocked(invokeTraced)
        .mock.calls.filter(([command]) => command === 'stop_pi')
        .map(([, args]) => (args as { runtimeId: string }).runtimeId),
    ).toEqual(['idle-runtime-2', 'idle-runtime-1']);
  });

  it('does not clear newer bookkeeping when an old transport send rejects', async () => {
    const { rpc } = await import('./index');
    const controller = makeController({
      postSettlementHydration: true,
      materializationMessagesRequestId: 'old-messages',
      syncing: true,
    });
    let rejectSend!: (error: Error) => void;
    mockInvoke.mockImplementationOnce(
      () =>
        new Promise<never>((_resolve, reject) => {
          rejectSend = reject;
        }),
    );

    const send = rpc(controller, {
      id: 'old-messages',
      type: 'get_messages',
    });
    controller.generation = 2;
    controller.sessionId = 'replacement';
    controller.sessionPath = '/tmp/project/replacement.jsonl';
    controller.materializationMessagesRequestId = 'new-messages';
    rejectSend(new Error('transport rejected'));

    await expect(send).rejects.toThrow('transport rejected');
    expect(controller.materializationMessagesRequestId).toBe('new-messages');
    expect(controller.syncing).toBe(true);
  });

  it('bounds a process failure without exposing raw bridge diagnostics', async () => {
    const {
      handleBridgeEvent,
      piConnectionFailureMessage,
      piProcessExitMessage,
    } = await import('./index');
    const controller = makeController({
      starting: true,
      streaming: true,
      working: true,
      stopping: true,
      messages: [
        { id: 'optimistic', kind: 'user', text: 'Keep this prompt' },
        { id: 'partial', kind: 'assistant', text: 'Keep this partial reply' },
      ],
    });
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'stderr',
      message: 'RAW_STDERR_SECRET_SENTINEL',
    });
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
      message: 'RAW_BRIDGE_ERROR_SECRET_SENTINEL',
    });
    expect(feedbackMessage(controller)).toBe(piConnectionFailureMessage);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 47,
      message: 'Pi exited with status 47. RAW_EXIT_SECRET_SENTINEL',
    });

    expect(controller).toMatchObject({
      generation: 0,
      ready: false,
      starting: false,
      streaming: false,
      stopping: false,
      working: false,
    });
    expect(feedbackMessage(controller)).toBe(piProcessExitMessage);
    expect(controller.messages.map(({ text }) => text)).toEqual([
      'Keep this prompt',
      'Keep this partial reply',
    ]);
    expect(JSON.stringify(controller)).not.toContain('RAW_');
  });

  it.each([1, 47, undefined])(
    'makes a direct local exit (%s) recoverable without an SSH notice',
    async (code) => {
      const { handleBridgeEvent } = await import('./index');
      const controller = makeController({
        draft: 'Newer draft',
        streaming: true,
        working: true,
        messages: [
          { id: 'partial', kind: 'assistant', text: 'Partial answer' },
          { id: 'tool', kind: 'tool', text: 'Inspecting', toolRunning: true },
        ],
      });
      state.controllers.push(controller);
      await handleBridgeEvent({
        runtimeId: controller.runtimeId,
        generation: 1,
        kind: 'exited',
        code,
      });
      expect(controller).toMatchObject({
        generation: 0,
        ready: false,
        streaming: false,
        working: false,
        runtimeDisconnected: true,
        recoveringRuntime: false,
        draft: 'Newer draft',
      });
      expect(controller.messages).toEqual([
        expect.objectContaining({ id: 'partial', text: 'Partial answer' }),
        expect.objectContaining({ id: 'tool', toolRunning: false }),
      ]);
      expect(
        controller.feedback.filter((entry) => !entry.acknowledged),
      ).toEqual([
        expect.objectContaining({
          action: 'reconnect',
          message: 'The Pi process stopped unexpectedly. Try again.',
        }),
      ]);
    },
  );

  it('retains local recovery when a transport failure is followed by exit zero', async () => {
    const { handleBridgeEvent } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: 1,
      kind: 'error',
    });
    expect(controller.runtimeDisconnected).toBe(true);
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: 1,
      kind: 'exited',
      code: 0,
    });
    expect(controller.runtimeDisconnected).toBe(true);
    expect(controller.feedback.filter((entry) => !entry.acknowledged)).toEqual([
      expect.objectContaining({
        action: 'reconnect',
        message: 'The Pi process stopped unexpectedly. Try again.',
      }),
    ]);
  });

  it('does not turn a clean local exit into a recovery incident', async () => {
    const { handleBridgeEvent } = await import('./index');
    const controller = makeController();
    state.controllers.push(controller);
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: 1,
      kind: 'exited',
      code: 0,
    });
    expect(controller.generation).toBe(0);
    expect(controller.runtimeDisconnected).toBe(false);
    expect(feedbackMessage(controller)).toBe('');
  });

  it.each(['get_state', 'get_messages'] as const)(
    'keeps local recovery retryable after a failed %s response',
    async (command) => {
      const { handleResponse, rpc } = await import('./index');
      const controller = makeController({
        ready: false,
        starting: true,
        runtimeDisconnected: true,
        recoveringRuntime: true,
        bootstrapStateRequestId: command === 'get_state' ? 'recovery-read' : '',
        startMessagesRequestId:
          command === 'get_messages' ? 'recovery-read' : '',
        draft: 'Keep draft',
      });
      state.controllers.push(controller);
      await rpc(controller, { id: 'recovery-read', type: command });
      await handleResponse(controller, {
        id: 'recovery-read',
        command,
        success: false,
      });
      await vi.waitFor(() => expect(controller.generation).toBe(0));
      expect(controller).toMatchObject({
        ready: false,
        starting: false,
        runtimeDisconnected: true,
        recoveringRuntime: false,
        draft: 'Keep draft',
      });
      expect(
        controller.feedback.filter((entry) => !entry.acknowledged),
      ).toEqual([
        expect.objectContaining({
          action: 'reconnect',
          message: 'The Pi connection failed. Try again.',
        }),
      ]);
      expect(state.remoteDialogOpen).toBe(false);
    },
  );

  it('does not probe an unwritten predecessor when recovery bootstrap reports a fresh identity', async () => {
    const { invokeTraced } = await import('../../telemetry');
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      ready: false,
      starting: true,
      runtimeDisconnected: true,
      recoveringRuntime: true,
      bootstrapStateRequestId: 'unwritten-bootstrap',
      bootstrapSessionPath: '',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      draft: 'Keep the newer draft',
      messages: [
        {
          id: 'history-user',
          kind: 'user',
          text: 'Earlier question',
          source: { message: 'previous-user', block: 0, offset: 0 },
        },
        {
          id: 'history-assistant',
          kind: 'assistant',
          text: 'Earlier answer',
          source: { message: 'previous-assistant', block: 0, offset: 0 },
        },
        {
          id: 'stream-assistant-1',
          kind: 'assistant',
          text: 'Interrupted reply',
        },
      ],
    });
    state.controllers.push(controller);
    state.activeControllerKey = controller.key;
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    const transcript = [...controller.messages];
    await rpc(controller, { id: 'unwritten-bootstrap', type: 'get_state' });

    await handleResponse(controller, {
      id: 'unwritten-bootstrap',
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'fresh-session',
        sessionFile: '/tmp/project/fresh-session.jsonl',
        isStreaming: false,
      },
    });

    expect(controller).toMatchObject({
      sessionId: 'fresh-session',
      sessionPath: '/tmp/project/fresh-session.jsonl',
      draft: 'Keep the newer draft',
    });
    expect(controller.messages).toEqual(transcript);
    expect(controller.startMessagesRequestId).not.toBe('');
    expect(state.activeSessionId).toBe('fresh-session');
    expect(state.activeSessionPath).toBe('/tmp/project/fresh-session.jsonl');
    expect(
      vi
        .mocked(invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session'),
    ).toEqual([]);
    expect(
      mockInvoke.mock.calls.filter(
        ([command]) => command === 'register_session',
      ),
    ).toEqual([]);
    expect(controller.feedback.filter((entry) => !entry.acknowledged)).toEqual(
      [],
    );
    expect(state.workspaceFeedback).toEqual([]);
  });

  it.each(['get_state', 'get_messages'] as const)(
    'retains runtime ownership and the recovery incident when failed %s cleanup cannot stop Pi',
    async (command) => {
      const { invokeTraced } = await import('../../telemetry');
      const { handleResponse, rpc, stopControllerProcess } =
        await import('./index');
      const controller = makeController({
        ready: false,
        starting: true,
        runtimeDisconnected: true,
        recoveringRuntime: true,
        bootstrapStateRequestId:
          command === 'get_state' ? 'failed-recovery' : '',
        startMessagesRequestId:
          command === 'get_messages' ? 'failed-recovery' : '',
        draft: 'Keep this draft',
        messages: [
          { id: 'partial', kind: 'assistant', text: 'Keep this reply' },
        ],
      });
      state.controllers.push(controller);
      vi.mocked(invokeTraced).mockRejectedValueOnce(
        new Error('RAW_STOP_FAILURE'),
      );
      await rpc(controller, { id: 'failed-recovery', type: command });
      await handleResponse(controller, {
        id: 'failed-recovery',
        command,
        success: false,
      });
      await vi.waitFor(() => expect(controller.recoveryStopFailed).toBe(true));
      expect(controller).toMatchObject({
        generation: 1,
        ready: false,
        starting: false,
        runtimeDisconnected: true,
        recoveringRuntime: false,
        recoveryStopFailed: true,
        draft: 'Keep this draft',
      });
      expect(controller.messages).toEqual([
        { id: 'partial', kind: 'assistant', text: 'Keep this reply' },
      ]);
      expect(
        controller.feedback.filter((entry) => !entry.acknowledged),
      ).toEqual([
        expect.objectContaining({
          action: 'reconnect',
          message: 'The Pi connection failed. Try again.',
        }),
      ]);
      expect(state.workspaceFeedback).toEqual([]);
      expect(JSON.stringify(controller)).not.toContain('RAW_STOP_FAILURE');
      expect(invokeTraced).toHaveBeenCalledWith(
        'stop_pi',
        expect.objectContaining({ runtimeId: controller.runtimeId }),
        undefined,
      );

      await stopControllerProcess(controller, undefined, false);
      expect(controller.generation).toBe(0);
      expect(controller.recoveryStopFailed).toBe(false);
      expect(controller.runtimeDisconnected).toBe(true);
      expect(feedbackMessage(controller)).toBe(
        'The Pi connection failed. Try again.',
      );
    },
  );

  it('turns an established remote exit into session-local recovery state', async () => {
    const { handleBridgeEvent, runtimeDisconnectedMessage } =
      await import('./index');
    const controller = makeController({
      ready: true,
      streaming: true,
      working: true,
      syncing: true,
      draft: 'Keep this newer draft',
      messages: [
        { id: 'stream-assistant-1', kind: 'assistant', text: 'Partial reply' },
        {
          id: 'stream-tool-2',
          kind: 'tool',
          text: 'Inspecting',
          toolRunning: true,
        },
      ],
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: null,
      projects: [
        {
          path: controller.projectPath,
          name: 'Remote project',
          workingDirectory: '/remote/project',
          connectionString: 'ssh fixture@example',
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 255,
    });

    expect(controller).toMatchObject({
      generation: 0,
      ready: false,
      streaming: false,
      working: false,
      syncing: false,
      connectingRemote: false,
      runtimeDisconnected: true,
      recoveringRuntime: false,
      draft: 'Keep this newer draft',
    });
    expect(feedbackMessage(controller)).toBe(runtimeDisconnectedMessage);
    expect(controller.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ kind: 'assistant', text: 'Partial reply' }),
        expect.objectContaining({
          kind: 'tool',
          text: 'Inspecting',
          toolRunning: false,
        }),
        expect.objectContaining({
          id: 'remote-interruption-1',
          kind: 'notice',
        }),
      ]),
    );
    expect(state.remoteDialogOpen).toBe(false);
    expect(state.remoteRetry).toBeUndefined();
  });

  it('keeps a failed explicit remote reconnect retryable in place', async () => {
    const { handleBridgeEvent, remoteReconnectFailureMessage } =
      await import('./index');
    const controller = makeController({
      ready: false,
      starting: true,
      connectingRemote: true,
      runtimeDisconnected: true,
      recoveringRuntime: true,
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: null,
      projects: [
        {
          path: controller.projectPath,
          name: 'Remote project',
          workingDirectory: '/remote/project',
          connectionString: 'ssh fixture@example',
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
    });
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 255,
    });

    expect(controller).toMatchObject({
      generation: 0,
      starting: false,
      connectingRemote: false,
      runtimeDisconnected: true,
      recoveringRuntime: false,
    });
    expect(feedbackMessage(controller)).toBe(remoteReconnectFailureMessage);
    expect(state.remoteDialogOpen).toBe(false);
    expect(state.remoteRetry).toBeUndefined();
  });

  it('makes a failed reconnect bootstrap response retryable', async () => {
    const { handleResponse, remoteReconnectFailureMessage, rpc } =
      await import('./index');
    const controller = makeController({
      ready: false,
      starting: true,
      connectingRemote: true,
      runtimeDisconnected: true,
      recoveringRuntime: true,
      bootstrapStateRequestId: 'reconnect-state',
    });
    state.controllers.push(controller);
    await rpc(controller, { id: 'reconnect-state', type: 'get_state' });

    await handleResponse(controller, {
      id: 'reconnect-state',
      command: 'get_state',
      success: false,
    });
    await vi.waitFor(() => expect(controller.generation).toBe(0));

    expect(controller).toMatchObject({
      ready: false,
      starting: false,
      runtimeDisconnected: true,
      recoveringRuntime: false,
    });
    expect(feedbackMessage(controller)).toBe(remoteReconnectFailureMessage);
    expect(state.remoteDialogOpen).toBe(false);
  });

  it('keeps the runtime state intact when process termination fails', async () => {
    const telemetry = await import('../../telemetry');
    const { stopControllerProcess } = await import('./index');
    const controller = makeController({
      generation: 1,
      ready: true,
      working: true,
      messages: [{ id: 'message-1', kind: 'assistant', text: 'Keep this' }],
    });
    vi.mocked(telemetry.invokeTraced).mockRejectedValueOnce(
      new Error('Pi did not stop in time. Try again.'),
    );

    await stopControllerProcess(controller);

    expect(controller.generation).toBe(1);
    expect(controller.ready).toBe(true);
    expect(controller.working).toBe(true);
    expect(controller.messages).toHaveLength(1);
    expect(feedbackMessage(controller)).toBe(
      'The Pi process could not be closed. Restart Tau and try again.',
    );
  });
});
