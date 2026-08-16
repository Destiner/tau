import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { parseTraceContext } from './trace-context';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({ accepted: 1, rejected: 0 })),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockClear();
  vi.resetModules();
});

describe('startCommandSpan', () => {
  it('returns a context that round-trips as a valid W3C traceparent', async () => {
    const { startCommandSpan } = await import('./index');
    const span = startCommandSpan('load_workspace');

    expect(span.context).toBeDefined();
    const context = span.context!;
    const traceparent = `00-${context.traceId}-${context.spanId}-01`;
    const parsed = parseTraceContext(traceparent);
    expect(parsed).toEqual({ ok: true, context });
  });

  it('gives concurrent spans independent trace context', async () => {
    const { startCommandSpan } = await import('./index');
    const first = startCommandSpan('load_workspace');
    const second = startCommandSpan('load_workspace');

    expect(first.context).toBeDefined();
    expect(second.context).toBeDefined();
    expect(first.context?.traceId).not.toBe(second.context?.traceId);
    expect(first.context?.spanId).not.toBe(second.context?.spanId);
  });

  it('flushes the ended span to the native ingest command', async () => {
    const { startCommandSpan, flushTelemetry } = await import('./index');
    const span = startCommandSpan('load_workspace');
    span.end();

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'tauri.invoke',
            traceId: span.context?.traceId,
            spanId: span.context?.spanId,
            attributes: { 'tau.invoke.command': 'load_workspace' },
          }),
        ],
      }),
    );
  });

  it('never asks the ingest command to trace itself', async () => {
    const { startCommandSpan, flushTelemetry } = await import('./index');
    startCommandSpan('load_workspace').end();
    await flushTelemetry();
    mockInvoke.mockClear();

    await flushTelemetry();

    expect(mockInvoke).not.toHaveBeenCalled();
  });
});
