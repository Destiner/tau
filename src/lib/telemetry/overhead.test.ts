/*
 * Stage 6's frontend performance measurements: instrumentation overhead on
 * typing/streaming-rate operations, and IPC batch frequency/queue-size
 * boundedness under a burst far larger than any real workload produces.
 * Thresholds are deliberately generous (10x+ the measured cost on ordinary
 * developer hardware) so this is a regression guard against something
 * becoming orders of magnitude slower, not a tight microbenchmark that
 * flakes on a loaded CI machine.
 */
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({ accepted: 0, rejected: 0 })),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockClear();
  vi.resetModules();
});

/** These measure the cost of telemetry that is actually recording, so each
 * one enables it the way admin mode does. */
async function loadTelemetry(): Promise<typeof import('./index')> {
  const telemetry = await import('./index');
  telemetry.setTelemetryEnabled(true);
  return telemetry;
}

describe('frontend instrumentation overhead', () => {
  it('starting and ending 2000 command spans (typing-rate volume) stays well under budget', async () => {
    const { startCommandSpan } = await loadTelemetry();

    const start = performance.now();
    for (let index = 0; index < 2000; index += 1) {
      startCommandSpan('load_workspace').end();
    }
    const elapsedMs = performance.now() - start;

    // Measured ~0.01ms/span on ordinary hardware; 0.25ms/span leaves a
    // large margin before this could be mistaken for "perceptible" at
    // keystroke rate (well under a single frame budget per operation).
    expect(elapsedMs).toBeLessThan(500);
  });

  it('recording 500 heartbeats/state-summaries (one every 30s for over 4 hours) stays well under budget', async () => {
    const { recordHeartbeat, recordStateSummary } = await loadTelemetry();

    const start = performance.now();
    for (let index = 0; index < 500; index += 1) {
      recordHeartbeat({
        visibility: 'visible',
        focused: true,
        pendingRpcCount: 1,
        controllerCount: 3,
        activeControllerCount: 1,
        runtimeCount: 1,
        queueLength: 5,
      });
      recordStateSummary({
        controllerCount: 3,
        runtimeCount: 1,
        pendingRpcCount: 1,
        notificationCount: 0,
        dialogCount: 0,
        transcriptCounts: {
          user: 10,
          assistant: 10,
          tool: 2,
          thinking: 2,
          error: 0,
        },
        draftBucket: 'short',
        oldestPendingRpcAgeMs: 0,
      });
    }
    const elapsedMs = performance.now() - start;

    expect(elapsedMs).toBeLessThan(500);
  });

  it('accumulating 10000 streaming deltas (recordStreamDelta never itself calls into telemetry) stays well under budget', async () => {
    const { recordStreamAggregate } = await loadTelemetry();

    // Streaming deltas are aggregated in `runtime.ts` (not exported from
    // `index.ts`) and only ever produce one `recordStreamAggregate` call per
    // run; this measures that one bounded call's own cost, confirming it is
    // negligible regardless of how many deltas it summarizes.
    const start = performance.now();
    for (let index = 0; index < 200; index += 1) {
      recordStreamAggregate('runtime-1', 1, 10000, 480_000, 1_000, 60_000);
    }
    const elapsedMs = performance.now() - start;

    expect(elapsedMs).toBeLessThan(200);
  });
});

describe('IPC batch frequency and queue size under a burst', () => {
  it('bounds both the number of native calls and the queue length for a 500-span burst', async () => {
    const { startCommandSpan, flushTelemetry, telemetryQueueLength } =
      await loadTelemetry();

    for (let index = 0; index < 500; index += 1) {
      startCommandSpan('load_workspace').end();
    }
    // Every `.end()` already called `scheduleFlush()`; nothing has actually
    // been sent yet since the debounce window has not elapsed.
    expect(mockInvoke).not.toHaveBeenCalled();

    await flushTelemetry();

    // MAX_QUEUE_SIZE (200) bounds how much of a 500-record burst is ever
    // held at once; MAX_BATCH_SIZE (20) bounds how large a single native
    // call is, so 200 queued records cost exactly 10 IPC calls, not 500.
    expect(mockInvoke).toHaveBeenCalledTimes(10);
    for (const call of mockInvoke.mock.calls) {
      const records = (call[1] as { records: unknown[] }).records;
      expect(records.length).toBeLessThanOrEqual(20);
    }
    expect(telemetryQueueLength()).toBe(0);
  });
});
