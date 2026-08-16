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

  it('nests under an explicit parent action context without ambient state', async () => {
    const { startActionSpan, startCommandSpan } = await import('./index');
    const action = startActionSpan('session.select');
    const invokeSpan = startCommandSpan('set_active_session', action.context);

    expect(invokeSpan.context?.traceId).toBe(action.context?.traceId);
    expect(invokeSpan.context?.spanId).not.toBe(action.context?.spanId);
  });

  it('persists an action with invoke and RPC children in one trace', async () => {
    const { flushTelemetry, startActionSpan, startCommandSpan, startRpcSpan } =
      await import('./index');
    const scope = {
      sessionId: 'session-1',
      controllerId: 'controller-1',
    };
    const action = startActionSpan('session.select', scope);
    const command = startCommandSpan('set_active_session', action.context);
    const rpc = startRpcSpan(
      'get_state',
      'tau-state-1',
      'runtime-1',
      1,
      action.context,
      scope,
    );
    command.end();
    rpc.end('success');
    action.end();

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([name]) => name === 'ingest_telemetry',
    );
    const records = (
      ingestCall?.[1] as {
        records: Array<{
          family: string;
          traceId: string;
          spanId: string;
          parentSpanId?: string;
        }>;
      }
    ).records;
    const actionRecord = records.find(
      (record) => record.family === 'ui.action',
    );
    const children = records.filter((record) => record.family !== 'ui.action');
    expect(actionRecord).toBeDefined();
    expect(children).toHaveLength(2);
    expect(
      children.every((record) => record.traceId === actionRecord?.traceId),
    ).toBe(true);
    expect(
      children.every((record) => record.parentSpanId === actionRecord?.spanId),
    ).toBe(true);
  });
});

describe('startActionSpan', () => {
  it('gives concurrent actions independent trace context', async () => {
    const { startActionSpan } = await import('./index');
    const first = startActionSpan('session.select');
    const second = startActionSpan('message.send');

    expect(first.context?.traceId).not.toBe(second.context?.traceId);
  });

  it('flushes with the reviewed action name attribute', async () => {
    const { startActionSpan, flushTelemetry } = await import('./index');
    startActionSpan('message.send', {
      sessionId: 'session-1',
      controllerId: 'controller-1',
    }).end();

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'ui.action',
            attributes: {
              'tau.action.name': 'message.send',
              'tau.session.id': 'session-1',
              'tau.controller.id': 'controller-1',
            },
          }),
        ],
      }),
    );
  });
});

describe('invokeTraced', () => {
  it('never serializes the command arguments into the span attributes', async () => {
    const { invokeTraced, flushTelemetry } = await import('./index');
    await invokeTraced('import_project', { path: '/tmp/tau-canary-project' });

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([command]) => command === 'ingest_telemetry',
    );
    const records = (
      ingestCall?.[1] as {
        records: Array<{ attributes: Record<string, unknown> }>;
      }
    ).records;
    expect(records).toEqual([
      expect.objectContaining({
        family: 'tauri.invoke',
        attributes: { 'tau.invoke.command': 'import_project' },
      }),
    ]);
  });

  it('passes the invoke call its own span context alongside the real arguments', async () => {
    const { invokeTraced } = await import('./index');
    await invokeTraced('import_project', { path: '/tmp/tau-canary-project' });

    expect(mockInvoke).toHaveBeenCalledWith(
      'import_project',
      expect.objectContaining({
        path: '/tmp/tau-canary-project',
        telemetryContext: expect.objectContaining({
          traceId: expect.any(String),
        }),
      }),
    );
  });

  it('ends the span even when the underlying invoke rejects', async () => {
    mockInvoke.mockRejectedValueOnce(new Error('tau-canary-invoke-error'));
    const { invokeTraced, flushTelemetry } = await import('./index');

    await expect(
      invokeTraced('remove_project', { path: '/tmp/tau-canary-project' }),
    ).rejects.toThrow('tau-canary-invoke-error');

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'tauri.invoke',
            attributes: { 'tau.invoke.command': 'remove_project' },
          }),
        ],
      }),
    );
  });
});

describe('startRpcSpan', () => {
  it('flushes with method, request id, runtime, and generation, but no outcome until ended', async () => {
    const { startRpcSpan, flushTelemetry } = await import('./index');
    const span = startRpcSpan(
      'prompt',
      'tau-prompt-1',
      'runtime-1',
      3,
      undefined,
      { sessionId: 'session-1', controllerId: 'controller-1' },
    );
    span.end('success');

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'pi.rpc',
            attributes: {
              'pi.rpc.method': 'prompt',
              'pi.rpc.request_id': 'tau-prompt-1',
              'tau.runtime.id': 'runtime-1',
              'pi.generation': 3,
              'pi.rpc.outcome': 'success',
              'tau.session.id': 'session-1',
              'tau.controller.id': 'controller-1',
            },
          }),
        ],
      }),
    );
  });

  it('drops an unreviewed outcome without failing to end the span', async () => {
    const { startRpcSpan, flushTelemetry } = await import('./index');
    const span = startRpcSpan('prompt', 'tau-prompt-1', 'runtime-1', 3);
    span.end('not-a-real-outcome' as never);

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([command]) => command === 'ingest_telemetry',
    );
    const records = (
      ingestCall?.[1] as {
        records: Array<{ attributes: Record<string, unknown> }>;
      }
    ).records;
    expect(records[0]?.attributes['pi.rpc.outcome']).toBeUndefined();
  });

  it('nests under an explicit parent action context', async () => {
    const { startActionSpan, startRpcSpan } = await import('./index');
    const action = startActionSpan('message.send');
    const rpc = startRpcSpan(
      'prompt',
      'tau-prompt-1',
      'runtime-1',
      3,
      action.context,
    );

    expect(rpc.context?.traceId).toBe(action.context?.traceId);
  });
});

describe('recordStreamAggregate', () => {
  it('records exactly one bounded aggregate span per call, never one per delta', async () => {
    const { recordStreamAggregate, flushTelemetry } = await import('./index');
    recordStreamAggregate('runtime-1', 3, 12, 480, 1_000, 1_500, {
      sessionId: 'session-1',
      controllerId: 'controller-1',
    });

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'pi.stream',
            attributes: {
              'pi.stream.delta_count': 12,
              'pi.stream.character_count': 480,
              'tau.runtime.id': 'runtime-1',
              'pi.generation': 3,
              'tau.session.id': 'session-1',
              'tau.controller.id': 'controller-1',
            },
          }),
        ],
      }),
    );
  });
});
