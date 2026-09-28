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

  it('turns an established remote exit into session-local recovery state', async () => {
    const { handleBridgeEvent, remoteDisconnectedMessage } =
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
      remoteDisconnected: true,
      reconnectingRemote: false,
      draft: 'Keep this newer draft',
    });
    expect(feedbackMessage(controller)).toBe(remoteDisconnectedMessage);
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
      remoteDisconnected: true,
      reconnectingRemote: true,
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
      remoteDisconnected: true,
      reconnectingRemote: false,
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
      remoteDisconnected: true,
      reconnectingRemote: true,
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
      remoteDisconnected: true,
      reconnectingRemote: false,
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
