import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({ accepted: 1, rejected: 0 })),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  mockInvoke.mockClear();
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn(() => 1),
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.stubGlobal('document', { visibilityState: 'visible' });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function records(): Promise<Array<Record<string, unknown>>> {
  return mockInvoke.mock.calls.flatMap(
    ([, args]) =>
      (args as { records?: Array<Record<string, unknown>> })?.records ?? [],
  );
}

describe('action milestones', () => {
  it('links fixed readiness, persistence and paint opportunity to the action once each', async () => {
    const telemetry = await import('./index');
    telemetry.setTelemetryEnabled(true);
    const action = telemetry.startActionMilestones('session.select', {
      sessionId: 's1',
    });
    action.mark('ready');
    action.mark('ready');
    action.mark('persisted');
    action.afterRender();
    const raf = vi.mocked(requestAnimationFrame);
    expect(raf).toHaveBeenCalledTimes(1);
    raf.mock.calls[0]![0](0);
    raf.mock.calls[1]![0](16);
    action.afterRender();
    action.span.end();
    await telemetry.flushTelemetry();

    const milestones = (await records()).filter(
      (r) => r.family === 'action.milestone',
    );
    expect(
      milestones.map(
        (r) =>
          (r.attributes as Record<string, unknown>)['tau.action.milestone'],
      ),
    ).toEqual(['ready', 'persisted', 'paint_opportunity']);
    expect(
      milestones.every(
        (r) =>
          r.traceId === action.span.context?.traceId &&
          r.spanId === action.span.context?.spanId,
      ),
    ).toBe(true);
    expect(
      milestones.every(
        (r) =>
          (r.attributes as Record<string, unknown>)['tau.session.id'] === 's1',
      ),
    ).toBe(true);
  });

  it('bounds a suspended frame and does not report a false paint', async () => {
    const telemetry = await import('./index');
    telemetry.setTelemetryEnabled(true);
    const action = telemetry.startActionMilestones('session.new');
    action.afterRender();
    await vi.advanceTimersByTimeAsync(5_000);
    vi.mocked(requestAnimationFrame).mock.calls[0]![0](0);
    await telemetry.flushTelemetry();
    const milestones = (await records()).filter(
      (r) => r.family === 'action.milestone',
    );
    expect(milestones).toHaveLength(1);
    expect(
      (milestones[0]?.attributes as Record<string, unknown>)[
        'tau.action.milestone'
      ],
    ).toBe('paint_unavailable');
  });

  it('marks hidden windows unavailable without waiting for a frame', async () => {
    const telemetry = await import('./index');
    telemetry.setTelemetryEnabled(true);
    vi.stubGlobal('document', { visibilityState: 'hidden' });
    telemetry.startActionMilestones('session.select').afterRender();
    await telemetry.flushTelemetry();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    const milestones = (await records()).filter(
      (r) => r.family === 'action.milestone',
    );
    expect(
      (milestones[0]?.attributes as Record<string, unknown>)[
        'tau.action.milestone'
      ],
    ).toBe('paint_unavailable');
  });

  it('cancels pending frames without recording paint', async () => {
    const telemetry = await import('./index');
    telemetry.setTelemetryEnabled(true);
    const action = telemetry.startActionMilestones('session.select');
    action.afterRender();
    action.cancel();
    await vi.advanceTimersByTimeAsync(5_000);
    await telemetry.flushTelemetry();
    expect(
      (await records()).filter((r) => r.family === 'action.milestone'),
    ).toHaveLength(0);
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });

  it('does not count a superseded selection between animation frames', async () => {
    const telemetry = await import('./index');
    telemetry.setTelemetryEnabled(true);
    const old = telemetry.startActionMilestones('session.select');
    old.afterRender();
    vi.mocked(requestAnimationFrame).mock.calls[0]![0](0);
    old.cancel();
    const next = telemetry.startActionMilestones('session.select');
    next.afterRender();
    // Even a callback already queued by the browser must not report the old view.
    vi.mocked(requestAnimationFrame).mock.calls[1]![0](16);
    vi.mocked(requestAnimationFrame).mock.calls[2]![0](16);
    vi.mocked(requestAnimationFrame).mock.calls[3]![0](32);
    await telemetry.flushTelemetry();
    const milestones = (await records()).filter(
      (r) => r.family === 'action.milestone',
    );
    expect(milestones).toHaveLength(1);
    expect(milestones[0]?.traceId).toBe(next.span.context?.traceId);
  });

  it('does not record or schedule frames when admin is off, or after it is disabled', async () => {
    const telemetry = await import('./index');
    const off = telemetry.startActionMilestones('session.select');
    off.mark('ready');
    off.afterRender();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    telemetry.setTelemetryEnabled(true);
    const action = telemetry.startActionMilestones('session.select');
    action.afterRender();
    telemetry.setTelemetryEnabled(false);
    telemetry.setTelemetryEnabled(true);
    vi.mocked(requestAnimationFrame).mock.calls[0]![0](0);
    vi.mocked(requestAnimationFrame).mock.calls[1]![0](16);
    action.mark('persisted');
    await telemetry.flushTelemetry();
    expect(
      (await records()).filter((r) => r.family === 'action.milestone'),
    ).toHaveLength(0);
  });
});
