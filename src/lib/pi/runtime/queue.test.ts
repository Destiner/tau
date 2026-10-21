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

  describe.each(['steer', 'followUp'] as const)(
    '%s during compaction',
    (kind) => {
      it.each(['threshold', 'overflow'] as const)(
        'queues during active automatic %s compaction',
        async (reason) => {
          const { submitQueuedMessage, handleRpc } = await import('./index');
          const controller = makeController({
            streaming: true,
            working: true,
            draft: '  queued during compaction\n',
            queueSteeringMode: 'all',
            queueFollowUpMode: 'one-at-a-time',
          });
          state.controllers.push(controller);
          await handleRpc(controller, { type: 'compaction_start', reason });
          const send = submitQueuedMessage(controller, controller.draft, kind);
          await vi.waitFor(() =>
            expect(requestOf('prompt')).toMatchObject({
              message: 'queued during compaction',
              streamingBehavior: kind,
            }),
          );
          await handleRpc(controller, {
            type: 'response',
            command: 'prompt',
            id: requestOf('prompt').id,
            success: true,
          });
          await send;
          expect(controller.draft).toBe('');
          expect(controller.queueSubmissions).toEqual([]);
          expect(controller.queueFeedback).toBe('');
          expect(controller.compacting).toBe(true);
          expect(controller.streaming).toBe(true);
          expect(controller.messages).toEqual([]);
          expect(mockInvoke.mock.calls).toHaveLength(1);
        },
      );

      it.each([
        ['manual', true],
        [undefined, true],
        ['invalid', true],
        ['threshold', false],
        ['overflow', false],
      ] as const)(
        'denies %s compaction started while streaming=%s without touching the draft',
        async (reason, startedWhileStreaming) => {
          const { submitQueuedMessage, handleRpc } = await import('./index');
          const controller = makeController({
            streaming: startedWhileStreaming,
          });
          state.controllers.push(controller);
          await handleRpc(controller, { type: 'compaction_start', reason });
          // A later stream must not make pre-run compaction queueable.
          controller.streaming = true;
          controller.draft = '  keep exact draft\n';
          await submitQueuedMessage(controller, controller.draft, kind);
          expect(controller.draft).toBe('  keep exact draft\n');
          expect(controller.queueSubmissions).toEqual([]);
          expect(controller.queueFailedDrafts).toEqual([]);
          expect(controller.queuePreparing).toBe(false);
          expect(mockInvoke).not.toHaveBeenCalled();
        },
      );

      describe.each([
        ['settlement', { streaming: false, working: false }],
        ['stopping', { stopping: true }],
        ['reconciliation', { compactionReconciliationPending: true }],
        ['manual compaction', { compactionReason: 'manual' }],
        ['unknown compaction', { compactionReason: undefined }],
        ['pre-run compaction', { compactionStartedWhileStreaming: false }],
      ] satisfies [string, Partial<SessionController>][])(
        'delayed preparation across %s',
        (_label, transition) => {
          it.each(['', 'newer draft'])(
            'cancels without a prompt and recovers the exact draft with newer draft=%j',
            async (newerDraft) => {
              const { submitQueuedMessage, handleRpc } =
                await import('./index');
              const original = '  exact unsent\n';
              const controller = makeController({
                streaming: true,
                working: true,
                compacting: true,
                compactionReason: 'threshold',
                compactionStartedWhileStreaming: true,
                draft: original,
              });
              state.controllers.push(controller);
              const send = submitQueuedMessage(controller, original, kind);
              expect(controller.draft).toBe('');
              expect(controller.queuePreparing).toBe(true);
              expect(controller.queueSubmissions).toHaveLength(1);
              await handleRpc(controller, {
                type: 'response',
                command: 'set_steering_mode',
                id: requestOf('set_steering_mode').id,
                success: true,
              });
              await vi.waitFor(() => requestOf('set_follow_up_mode'));
              Object.assign(controller, transition, { draft: newerDraft });
              await handleRpc(controller, {
                type: 'response',
                command: 'set_follow_up_mode',
                id: requestOf('set_follow_up_mode').id,
                success: true,
              });
              await send;
              expect(controller.draft).toBe(newerDraft || original);
              expect(controller.queueFailedDrafts).toEqual(
                newerDraft ? [original] : [],
              );
              expect(controller.queueSubmissions).toEqual([]);
              expect(controller.queuePreparing).toBe(false);
              expect(controller.queueFeedback).toContain('state changed');
              expect(controller.messages).toEqual([]);
              expect(
                mockInvoke.mock.calls.map(
                  ([, args]) =>
                    (args as { request: { type: string } }).request.type,
                ),
              ).toEqual(['set_steering_mode', 'set_follow_up_mode']);
            },
          );
        },
      );

      it.each(['threshold', 'overflow', 'finished'] as const)(
        'still queues when delayed mode preparation completes during %s compaction state',
        async (phase) => {
          const { submitQueuedMessage, handleRpc } = await import('./index');
          const controller = makeController({
            streaming: true,
            working: true,
            draft: 'queued',
          });
          state.controllers.push(controller);
          const send = submitQueuedMessage(controller, controller.draft, kind);
          await handleRpc(controller, {
            type: 'compaction_start',
            reason: phase === 'finished' ? 'threshold' : phase,
          });
          await handleRpc(controller, {
            type: 'response',
            command: 'set_steering_mode',
            id: requestOf('set_steering_mode').id,
            success: true,
          });
          await vi.waitFor(() => requestOf('set_follow_up_mode'));
          if (phase === 'finished') {
            Object.assign(controller, {
              compacting: false,
              compactionReason: undefined,
              compactionStartedWhileStreaming: false,
            });
          }
          await handleRpc(controller, {
            type: 'response',
            command: 'set_follow_up_mode',
            id: requestOf('set_follow_up_mode').id,
            success: true,
          });
          await vi.waitFor(() =>
            expect(requestOf('prompt')).toMatchObject({
              message: 'queued',
              streamingBehavior: kind,
            }),
          );
          await handleRpc(controller, {
            type: 'response',
            command: 'prompt',
            id: requestOf('prompt').id,
            success: true,
          });
          await send;
          expect(controller.queuePreparing).toBe(false);
          expect(controller.queueSubmissions).toEqual([]);
          expect(controller.queueFailedDrafts).toEqual([]);
          expect(controller.draft).toBe('');
          expect(controller.messages).toEqual([]);
          expect(mockInvoke.mock.calls).toHaveLength(3);
        },
      );

      it('does not replay identical text consumed before its acknowledgement', async () => {
        const { submitQueuedMessage, handleRpc } = await import('./index');
        const key = kind === 'steer' ? 'steering' : 'followUp';
        const controller = makeController({
          streaming: true,
          compacting: true,
          compactionReason: 'overflow',
          compactionStartedWhileStreaming: true,
          queueSteeringMode: 'all',
          queueFollowUpMode: 'one-at-a-time',
          queue: { steering: [], followUp: [], [key]: ['same'] },
          draft: 'same',
        });
        state.controllers.push(controller);
        const send = submitQueuedMessage(controller, controller.draft, kind);
        await vi.waitFor(() => requestOf('prompt'));
        const id = requestOf('prompt').id;
        for (const items of [['same', 'same'], ['same'], []]) {
          await handleRpc(controller, {
            type: 'queue_update',
            steering: [],
            followUp: [],
            [key]: items,
          });
          expect(controller.queue[key]).toEqual(items);
        }
        expect(controller.queueSubmissions).toHaveLength(1);
        await handleRpc(controller, {
          type: 'response',
          command: 'prompt',
          id,
          success: true,
        });
        await send;
        // Neither the acknowledgement nor a duplicate can resurrect consumed text.
        await handleRpc(controller, {
          type: 'response',
          command: 'prompt',
          id,
          success: true,
        });
        expect(controller.queue).toEqual({ steering: [], followUp: [] });
        expect(controller.queueSubmissions).toEqual([]);
        expect(controller.queueFailedDrafts).toEqual([]);
        expect(controller.draft).toBe('');
        expect(controller.messages).toEqual([]);
        expect(mockInvoke.mock.calls).toHaveLength(1);
      });
    },
  );

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
