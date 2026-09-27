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

describe('queued message RPCs', () => {
  function requestOf(type: string): Record<string, unknown> {
    const call = [...mockInvoke.mock.calls]
      .reverse()
      .find(
        ([method, args]) =>
          method === 'send_pi' &&
          (args as { request: { type: string } }).request.type === type,
      );
    if (!call) throw new Error(`Missing ${type} request`);
    return (call[1] as { request: Record<string, unknown> }).request;
  }

  it('prepares modes lazily, correlates replies, and reconciles snapshots before acknowledgement', async () => {
    const { submitQueuedMessage, handleRpc } = await import('./index');
    const controller = makeController({
      streaming: true,
      working: true,
      draft: 'same',
    });
    state.controllers.push(controller);
    const send = submitQueuedMessage(controller, 'same', 'steer');
    expect(controller.draft).toBe('');
    expect(controller.queueSubmissions).toHaveLength(1);
    await vi.waitFor(() =>
      expect(requestOf('set_steering_mode').mode).toBe('all'),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'set_steering_mode',
      id: requestOf('set_steering_mode').id,
      success: true,
    });
    await vi.waitFor(() =>
      expect(requestOf('set_follow_up_mode').mode).toBe('one-at-a-time'),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'set_follow_up_mode',
      id: requestOf('set_follow_up_mode').id,
      success: true,
    });
    await vi.waitFor(() =>
      expect(requestOf('prompt').streamingBehavior).toBe('steer'),
    );
    await handleRpc(controller, {
      type: 'queue_update',
      steering: ['same'],
      followUp: [],
    });
    expect(controller.queue.steering).toEqual(['same']);
    await handleRpc(controller, {
      type: 'response',
      command: 'prompt',
      id: requestOf('prompt').id,
      success: true,
    });
    await send;
    expect(controller.queueSubmissions).toEqual([]);
    expect(controller.queue.steering).toEqual(['same']);
    expect(controller.streaming).toBe(true);
    expect(controller.messages).toEqual([]);
  });

  it('preserves a newer draft after a rejected follow-up and does not schedule an ordinary prompt', async () => {
    const { submitQueuedMessage, handleRpc } = await import('./index');
    const controller = makeController({
      streaming: true,
      working: true,
      queueSteeringMode: 'all',
      queueFollowUpMode: 'one-at-a-time',
      draft: 'first',
    });
    state.controllers.push(controller);
    const send = submitQueuedMessage(controller, 'first', 'followUp');
    controller.draft = 'newer';
    await vi.waitFor(() =>
      expect(requestOf('prompt').streamingBehavior).toBe('followUp'),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'prompt',
      id: requestOf('prompt').id,
      success: false,
    });
    await send;
    expect(controller.draft).toBe('newer');
    expect(controller.queueFailedDrafts).toEqual(['first']);
    expect(controller.streaming).toBe(true);
    expect(controller.messages).toEqual([]);
  });

  it('clears without aborting, ignores duplicate clear, and keeps a concurrent draft', async () => {
    const { clearPendingQueue, handleRpc } = await import('./index');
    const controller = makeController({
      streaming: true,
      queue: { steering: ['a'], followUp: ['b'] },
      draft: 'typing',
    });
    state.controllers.push(controller);
    const clear = clearPendingQueue(controller);
    void clearPendingQueue(controller);
    await vi.waitFor(() =>
      expect(requestOf('clear_queue').type).toBe('clear_queue'),
    );
    expect(
      mockInvoke.mock.calls.filter(([method]) => method === 'send_pi'),
    ).toHaveLength(1);
    await handleRpc(controller, {
      type: 'queue_update',
      steering: [],
      followUp: [],
    });
    await handleRpc(controller, {
      type: 'response',
      command: 'clear_queue',
      id: requestOf('clear_queue').id,
      success: true,
      data: { steering: ['a'], followUp: ['b'] },
    });
    await clear;
    expect(controller.queue).toEqual({ steering: [], followUp: [] });
    expect(controller.draft).toBe('typing');
    expect(controller.streaming).toBe(true);
  });

  it('keeps both queues when Clear All is rejected and permits retry', async () => {
    const { clearPendingQueue, handleRpc } = await import('./index');
    const controller = makeController({
      queue: { steering: ['one'], followUp: ['two'] },
      draft: 'new draft',
    });
    state.controllers.push(controller);
    const first = clearPendingQueue(controller);
    await vi.waitFor(() =>
      expect(requestOf('clear_queue').type).toBe('clear_queue'),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'clear_queue',
      id: requestOf('clear_queue').id,
      success: false,
    });
    await first;
    expect(controller.queue).toEqual({ steering: ['one'], followUp: ['two'] });
    expect(controller.draft).toBe('new draft');
    expect(controller.queueFeedback).toContain('Could not clear');
    const retry = clearPendingQueue(controller);
    await vi.waitFor(() =>
      expect(
        mockInvoke.mock.calls.filter(
          ([method, args]) =>
            method === 'send_pi' &&
            (args as { request: { type: string } }).request.type ===
              'clear_queue',
        ),
      ).toHaveLength(2),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'clear_queue',
      id: requestOf('clear_queue').id,
      success: true,
    });
    await retry;
    expect(controller.queue).toEqual({ steering: [], followUp: [] });
  });

  it('does not queue when mode preparation is rejected, restoring the exact unsent draft', async () => {
    const { submitQueuedMessage, handleRpc } = await import('./index');
    const controller = makeController({
      streaming: true,
      working: true,
      draft: '  exact message  ',
    });
    state.controllers.push(controller);
    const send = submitQueuedMessage(controller, controller.draft, 'steer');
    await vi.waitFor(() =>
      expect(requestOf('set_steering_mode').type).toBe('set_steering_mode'),
    );
    await handleRpc(controller, {
      type: 'response',
      command: 'set_steering_mode',
      id: requestOf('set_steering_mode').id,
      success: false,
    });
    await send;
    expect(controller.draft).toBe('  exact message  ');
    expect(controller.queueSubmissions).toEqual([]);
    expect(
      mockInvoke.mock.calls.some(
        ([method, args]) =>
          method === 'send_pi' &&
          (args as { request: { type: string } }).request.type === 'prompt',
      ),
    ).toBe(false);
  });

  it('does not replay a submission or adopt its late acknowledgement after process exit', async () => {
    const { submitQueuedMessage, handleBridgeEvent, handleRpc } =
      await import('./index');
    const controller = makeController({
      streaming: true,
      working: true,
      queueSteeringMode: 'all',
      queueFollowUpMode: 'one-at-a-time',
      draft: 'unsent',
    });
    state.controllers.push(controller);
    const send = submitQueuedMessage(controller, 'unsent', 'steer');
    await vi.waitFor(() =>
      expect(requestOf('prompt').streamingBehavior).toBe('steer'),
    );
    const id = requestOf('prompt').id;
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: 1,
      kind: 'exited',
      code: 0,
    });
    await send;
    expect(controller.generation).toBe(0);
    expect(controller.draft).toBe('unsent');
    expect(controller.queue).toEqual({ steering: [], followUp: [] });
    await handleRpc(controller, {
      type: 'response',
      command: 'prompt',
      id,
      success: true,
    });
    expect(controller.queue).toEqual({ steering: [], followUp: [] });
    expect(
      mockInvoke.mock.calls.filter(
        ([method, args]) =>
          method === 'send_pi' &&
          (args as { request: { type: string } }).request.type === 'prompt',
      ),
    ).toHaveLength(1);
  });
});
