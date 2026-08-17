/*
 * Stage 5's heartbeat/state-summary timer and long-task observer. The
 * recording functions themselves (`recordHeartbeat`/`recordEventLoopLag`/
 * `recordLongTask`/`recordStateSummary`) are covered by
 * `src/lib/telemetry/index.test.ts`, and `buildStateSnapshot` by
 * `src/composables/state.test.ts`; this file only tests this module's own
 * scheduling/observation mechanics, so every dependency below is mocked.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const stateSnapshot = {
  controllerCount: 2,
  runtimeCount: 1,
  activeControllerCount: 1,
  notificationCount: 0,
  dialogCount: 0,
  transcriptCounts: { user: 1, assistant: 1, tool: 0, thinking: 0, error: 0 },
  draftBucket: 'empty' as const,
  scope: { sessionId: 'session-1', controllerId: 'controller-1' },
};

vi.mock('../../composables/state', () => ({
  buildStateSnapshot: vi.fn(() => stateSnapshot),
}));

vi.mock('../pi/runtime', () => ({
  pendingRpcCount: vi.fn(() => 3),
  oldestPendingRpcAgeMs: vi.fn(() => 1234),
}));

const {
  recordEventLoopLag,
  recordHeartbeat,
  recordLongTask,
  recordStateSummary,
} = vi.hoisted(() => ({
  recordEventLoopLag: vi.fn(),
  recordHeartbeat: vi.fn(),
  recordLongTask: vi.fn(),
  recordStateSummary: vi.fn(),
}));

vi.mock('./index', () => ({
  recordEventLoopLag,
  recordHeartbeat,
  recordLongTask,
  recordStateSummary,
  telemetryQueueLength: (): number => 5,
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.resetModules();
});

afterEach(async () => {
  const { stopHeartbeat, stopLongTaskObserver } = await import('./heartbeat');
  stopHeartbeat();
  stopLongTaskObserver();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('startHeartbeat', () => {
  it('records a heartbeat and state summary on the first tick while visible and focused', async () => {
    vi.stubGlobal('document', {
      visibilityState: 'visible',
      hasFocus: () => true,
    });
    const { startHeartbeat } = await import('./heartbeat');

    startHeartbeat();
    await vi.advanceTimersByTimeAsync(30_000);

    expect(recordHeartbeat).toHaveBeenCalledWith(
      expect.objectContaining({
        visibility: 'visible',
        focused: true,
        pendingRpcCount: 3,
        controllerCount: 2,
        activeControllerCount: 1,
        runtimeCount: 1,
        queueLength: 5,
      }),
    );
    expect(recordStateSummary).toHaveBeenCalledWith(
      expect.objectContaining({
        controllerCount: 2,
        pendingRpcCount: 3,
        oldestPendingRpcAgeMs: 1234,
        draftBucket: 'empty',
        scope: { sessionId: 'session-1', controllerId: 'controller-1' },
      }),
    );
    expect(recordEventLoopLag).toHaveBeenCalledWith(
      expect.any(Number),
      'visible',
      true,
    );
  });

  it('reports hidden visibility and unfocused state without treating either as a failure', async () => {
    vi.stubGlobal('document', {
      visibilityState: 'hidden',
      hasFocus: () => false,
    });
    const { startHeartbeat } = await import('./heartbeat');

    startHeartbeat();
    await vi.advanceTimersByTimeAsync(30_000);

    expect(recordHeartbeat).toHaveBeenCalledWith(
      expect.objectContaining({ visibility: 'hidden', focused: false }),
    );
    expect(recordEventLoopLag).toHaveBeenCalledWith(
      expect.any(Number),
      'hidden',
      false,
    );
    // Neither reporting function itself throws or otherwise signals a
    // failure just because the window was hidden: the visibility/focus
    // dimensions are the evidence, not a separate error record.
    expect(recordHeartbeat).toHaveBeenCalledTimes(1);
  });

  it('reports a large lag reading when a tick actually fires later than scheduled', async () => {
    vi.stubGlobal('document', {
      visibilityState: 'hidden',
      hasFocus: () => false,
    });
    let now = 1_000_000;
    const nowSpy = vi.spyOn(Date, 'now').mockImplementation(() => now);

    const { startHeartbeat } = await import('./heartbeat');
    startHeartbeat();

    // Simulate background-timer throttling: the scheduled 30s tick actually
    // fires much later than its own schedule.
    now += 150_000;
    await vi.advanceTimersByTimeAsync(30_000);

    const call = recordEventLoopLag.mock.calls[0] as [number, string, boolean];
    expect(call[0]).toBeGreaterThanOrEqual(90_000);
    expect(call[1]).toBe('hidden');
    nowSpy.mockRestore();
  });

  it('is a no-op the second time it is called while already running', async () => {
    vi.stubGlobal('document', {
      visibilityState: 'visible',
      hasFocus: () => true,
    });
    const { startHeartbeat } = await import('./heartbeat');
    startHeartbeat();
    startHeartbeat();

    await vi.advanceTimersByTimeAsync(30_000);

    expect(recordHeartbeat).toHaveBeenCalledTimes(1);
  });

  it('reschedules and ticks again after stopping and restarting', async () => {
    vi.stubGlobal('document', {
      visibilityState: 'visible',
      hasFocus: () => true,
    });
    const { startHeartbeat, stopHeartbeat } = await import('./heartbeat');
    startHeartbeat();
    await vi.advanceTimersByTimeAsync(30_000);
    stopHeartbeat();
    vi.clearAllMocks();

    await vi.advanceTimersByTimeAsync(60_000);
    expect(recordHeartbeat).not.toHaveBeenCalled();

    startHeartbeat();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(recordHeartbeat).toHaveBeenCalledTimes(1);
  });
});

describe('installLongTaskObserver', () => {
  it('records each observed long task duration as a metric, never a log', async () => {
    const entries = [{ duration: 120 }, { duration: 80 }];
    const observeSpy = vi.fn();
    let deliver:
      ((list: { getEntries: () => typeof entries }) => void) | undefined;
    class FakePerformanceObserver {
      static supportedEntryTypes = ['longtask'];
      constructor(
        callback: (list: { getEntries: () => typeof entries }) => void,
      ) {
        deliver = callback;
      }
      observe(): void {
        observeSpy();
      }
      disconnect(): void {}
    }
    vi.stubGlobal('PerformanceObserver', FakePerformanceObserver);

    const { installLongTaskObserver } = await import('./heartbeat');
    installLongTaskObserver();
    deliver?.({ getEntries: () => entries });

    expect(observeSpy).toHaveBeenCalled();
    expect(recordLongTask).toHaveBeenCalledTimes(2);
    expect(recordLongTask).toHaveBeenCalledWith(120);
    expect(recordLongTask).toHaveBeenCalledWith(80);
  });

  it('does nothing when the webview does not support longtask entries', async () => {
    class FakePerformanceObserver {
      static supportedEntryTypes: string[] = [];
      observe(): void {
        throw new Error('must not be called when unsupported');
      }
      disconnect(): void {}
    }
    vi.stubGlobal('PerformanceObserver', FakePerformanceObserver);

    const { installLongTaskObserver } = await import('./heartbeat');
    expect(() => installLongTaskObserver()).not.toThrow();
  });

  it('does nothing when PerformanceObserver does not exist at all', async () => {
    vi.stubGlobal('PerformanceObserver', undefined);

    const { installLongTaskObserver } = await import('./heartbeat');
    expect(() => installLongTaskObserver()).not.toThrow();
  });
});
