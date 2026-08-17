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
        attributes: {
          'tau.invoke.command': 'import_project',
          'tau.invoke.outcome': 'success',
        },
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
            attributes: {
              'tau.invoke.command': 'remove_project',
              'tau.invoke.outcome': 'error',
            },
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

interface IngestedRecord {
  family: string;
  attributes: Record<string, unknown>;
}

function flushedRecords(): IngestedRecord[] {
  return mockInvoke.mock.calls
    .filter(([name]) => name === 'ingest_telemetry')
    .flatMap(([, args]) => (args as { records: IngestedRecord[] }).records);
}

describe('vueErrorHandler', () => {
  it('records the bounded error kind and a sanitized location, never the message', async () => {
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
    const { vueErrorHandler, flushTelemetry } = await import('./index');
    const error = new TypeError('tau-canary-vue-error-message');
    error.stack =
      'TypeError: tau-canary-vue-error-message\n    at run (/tmp/project/src/app.ts:5:2)';

    vueErrorHandler(error);

    await flushTelemetry();

    const record = flushedRecords().find(
      (candidate) => candidate.family === 'frontend.error',
    );
    expect(record).toEqual(
      expect.objectContaining({
        attributes: {
          'tau.error.source': 'vue_error',
          'tau.error.kind': 'TypeError',
          'tau.error.location': 'app.ts:5:2',
        },
      }),
    );
    expect(JSON.stringify(record)).not.toContain(
      'tau-canary-vue-error-message',
    );
    expect(consoleError).toHaveBeenCalledWith(error);
    consoleError.mockRestore();
  });

  it('still records a bounded record for a non-Error thrown value', async () => {
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
    const { vueErrorHandler, flushTelemetry } = await import('./index');

    vueErrorHandler('a plain string reason');

    await flushTelemetry();

    const record = flushedRecords().find(
      (candidate) => candidate.family === 'frontend.error',
    );
    expect(record?.attributes).toEqual({
      'tau.error.source': 'vue_error',
      'tau.error.kind': 'other',
      'tau.error.location': '',
    });
    expect(consoleError).toHaveBeenCalledWith('a plain string reason');
    consoleError.mockRestore();
  });
});

describe('installFrontendErrorCapture', () => {
  it('wraps console.error, still calling the original, and records a sanitized entry', async () => {
    const nativeConsoleError = vi.fn();

    console.error = nativeConsoleError;
    const { installFrontendErrorCapture, flushTelemetry } =
      await import('./index');
    installFrontendErrorCapture();
    installFrontendErrorCapture();

    const error = new RangeError('tau-canary-console-error-message');
    error.stack =
      'RangeError: tau-canary-console-error-message\n    at run (/tmp/project/src/console.ts:1:1)';

    console.error(error);

    expect(nativeConsoleError).toHaveBeenCalledWith(error);

    await flushTelemetry();

    const record = flushedRecords().find(
      (candidate) => candidate.family === 'frontend.error',
    );
    expect(record?.attributes).toEqual({
      'tau.error.source': 'console_error',
      'tau.error.kind': 'RangeError',
      'tau.error.location': 'console.ts:1:1',
    });
  });

  it('does not record a nested console.error call triggered by its own classification code', async () => {
    const nativeConsoleError = vi.fn();

    console.error = nativeConsoleError;
    const { installFrontendErrorCapture, flushTelemetry } =
      await import('./index');
    installFrontendErrorCapture();

    const reentrant = new Error('boom');
    Object.defineProperty(reentrant, 'stack', {
      get(): string {
        // Simulates a bug (or an unrelated integration) that reacts to the
        // original console.error call by logging again, synchronously,
        // before this call has finished recording its own telemetry.

        console.error('a reentrant console.error call');
        return 'Error: boom\n    at run (/tmp/project/src/a.ts:1:1)';
      },
    });

    console.error(reentrant);

    await flushTelemetry();

    const records = flushedRecords().filter(
      (candidate) => candidate.family === 'frontend.error',
    );
    expect(records).toHaveLength(1);
  });
});

describe('queue overflow reporting', () => {
  it('reports a telemetry.health log once the bounded queue starts dropping records', async () => {
    const { startCommandSpan, flushTelemetry } = await import('./index');

    for (let index = 0; index < 205; index += 1) {
      startCommandSpan('load_workspace').end();
    }

    await flushTelemetry();

    const healthRecord = flushedRecords().find(
      (candidate) => candidate.family === 'telemetry.health',
    );
    expect(healthRecord).toBeDefined();
    expect(healthRecord?.attributes['tau.telemetry.dropped_count']).toEqual(
      expect.any(Number),
    );
    expect(
      healthRecord?.attributes['tau.telemetry.dropped_count'] as number,
    ).toBeGreaterThan(0);
  });

  it('does not report the same drop count twice', async () => {
    const { startCommandSpan, flushTelemetry } = await import('./index');

    for (let index = 0; index < 201; index += 1) {
      startCommandSpan('load_workspace').end();
    }
    await flushTelemetry();
    mockInvoke.mockClear();

    startCommandSpan('load_workspace').end();
    await flushTelemetry();

    const healthRecords = flushedRecords().filter(
      (candidate) => candidate.family === 'telemetry.health',
    );
    expect(healthRecords).toHaveLength(0);
  });
});
