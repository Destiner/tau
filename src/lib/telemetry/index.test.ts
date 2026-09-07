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

/** Telemetry records nothing until admin mode turns it on, so every test
 * that expects records to reach the native command enables it first. */
async function loadTelemetry(): Promise<typeof import('./index')> {
  const telemetry = await import('./index');
  telemetry.setTelemetryEnabled(true);
  return telemetry;
}

describe('startCommandSpan', () => {
  it('returns a context that round-trips as a valid W3C traceparent', async () => {
    const { startCommandSpan } = await loadTelemetry();
    const span = startCommandSpan('load_workspace');

    expect(span.context).toBeDefined();
    const context = span.context!;
    const traceparent = `00-${context.traceId}-${context.spanId}-01`;
    const parsed = parseTraceContext(traceparent);
    expect(parsed).toEqual({ ok: true, context });
  });

  it('gives concurrent spans independent trace context', async () => {
    const { startCommandSpan } = await loadTelemetry();
    const first = startCommandSpan('load_workspace');
    const second = startCommandSpan('load_workspace');

    expect(first.context).toBeDefined();
    expect(second.context).toBeDefined();
    expect(first.context?.traceId).not.toBe(second.context?.traceId);
    expect(first.context?.spanId).not.toBe(second.context?.spanId);
  });

  it('flushes the ended span to the native ingest command', async () => {
    const { startCommandSpan, flushTelemetry } = await loadTelemetry();
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
    const { startCommandSpan, flushTelemetry } = await loadTelemetry();
    startCommandSpan('load_workspace').end();
    await flushTelemetry();
    mockInvoke.mockClear();

    await flushTelemetry();

    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it('nests under an explicit parent action context without ambient state', async () => {
    const { startActionSpan, startCommandSpan } = await loadTelemetry();
    const action = startActionSpan('session.select');
    const invokeSpan = startCommandSpan('set_active_session', action.context);

    expect(invokeSpan.context?.traceId).toBe(action.context?.traceId);
    expect(invokeSpan.context?.spanId).not.toBe(action.context?.spanId);
  });

  it('persists an action with invoke and RPC children in one trace', async () => {
    const { flushTelemetry, startActionSpan, startCommandSpan, startRpcSpan } =
      await loadTelemetry();
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

    const records = flushedRecords();
    const actionRecord = records.find(
      (record) => record.family === 'ui.action',
    );
    // Two real span children (the invoke and the RPC) plus one linked
    // `operation.checkpoint` log per checkpointed family (`ui.action` and
    // `pi.rpc`; `tauri.invoke` is not checkpointed) — five records sharing
    // one trace in total.
    const spanChildren = records.filter(
      (record) =>
        record.family === 'tauri.invoke' || record.family === 'pi.rpc',
    );
    const checkpoints = records.filter(
      (record) => record.family === 'operation.checkpoint',
    );
    expect(actionRecord).toBeDefined();
    expect(spanChildren).toHaveLength(2);
    expect(checkpoints).toHaveLength(2);
    expect(
      records.every((record) => record.traceId === actionRecord?.traceId),
    ).toBe(true);
    expect(
      spanChildren.every(
        (record) => record.parentSpanId === actionRecord?.spanId,
      ),
    ).toBe(true);
  });
});

describe('startActionSpan', () => {
  it('gives concurrent actions independent trace context', async () => {
    const { startActionSpan } = await loadTelemetry();
    const first = startActionSpan('session.select');
    const second = startActionSpan('message.send');

    expect(first.context?.traceId).not.toBe(second.context?.traceId);
  });

  it('flushes with the reviewed action name attribute', async () => {
    const { startActionSpan, flushTelemetry } = await loadTelemetry();
    startActionSpan('message.send', {
      sessionId: 'session-1',
      controllerId: 'controller-1',
    }).end();

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: expect.arrayContaining([
          expect.objectContaining({
            family: 'ui.action',
            attributes: {
              'tau.action.name': 'message.send',
              'tau.session.id': 'session-1',
              'tau.controller.id': 'controller-1',
            },
          }),
        ]),
      }),
    );
  });
});

describe('invokeTraced', () => {
  it('never serializes the command arguments into the span attributes', async () => {
    const { invokeTraced, flushTelemetry } = await loadTelemetry();
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
    const { invokeTraced } = await loadTelemetry();
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
    const { invokeTraced, flushTelemetry } = await loadTelemetry();

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
    const { startRpcSpan, flushTelemetry } = await loadTelemetry();
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
        records: expect.arrayContaining([
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
        ]),
      }),
    );
  });

  it('drops an unreviewed outcome without failing to end the span', async () => {
    const { startRpcSpan, flushTelemetry } = await loadTelemetry();
    const span = startRpcSpan('prompt', 'tau-prompt-1', 'runtime-1', 3);
    span.end('not-a-real-outcome' as never);

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([command]) => command === 'ingest_telemetry',
    );
    const records = (
      ingestCall?.[1] as {
        records: Array<{ family: string; attributes: Record<string, unknown> }>;
      }
    ).records;
    const rpcRecord = records.find((record) => record.family === 'pi.rpc');
    expect(rpcRecord?.attributes['pi.rpc.outcome']).toBeUndefined();
  });

  it('nests under an explicit parent action context', async () => {
    const { startActionSpan, startRpcSpan } = await loadTelemetry();
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

describe('operation checkpoints', () => {
  it('records a ui.action checkpoint immediately, even if the span never ends', async () => {
    const { startActionSpan, flushTelemetry } = await loadTelemetry();
    const action = startActionSpan('message.send');

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
          attributes: Record<string, unknown>;
        }>;
      }
    ).records;
    const checkpoint = records.find(
      (record) => record.family === 'operation.checkpoint',
    );
    expect(checkpoint).toBeDefined();
    expect(checkpoint?.attributes).toEqual({
      'tau.operation.family': 'ui.action',
      'tau.operation.name': 'message.send',
    });
    expect(checkpoint?.traceId).toBe(action.context?.traceId);
    expect(checkpoint?.spanId).toBe(action.context?.spanId);
    // The action's own span is never ended in this test: only the linked
    // checkpoint log proves the operation began.
    expect(records.some((record) => record.family === 'ui.action')).toBe(false);
  });

  it('records a pi.rpc checkpoint immediately, even if the span never ends', async () => {
    const { startRpcSpan, flushTelemetry } = await loadTelemetry();
    const rpc = startRpcSpan('prompt', 'tau-prompt-1', 'runtime-1', 1);

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
          attributes: Record<string, unknown>;
        }>;
      }
    ).records;
    const checkpoint = records.find(
      (record) => record.family === 'operation.checkpoint',
    );
    expect(checkpoint).toBeDefined();
    expect(checkpoint?.attributes).toEqual({
      'tau.operation.family': 'pi.rpc',
      'tau.operation.name': 'prompt',
      'pi.rpc.request_id': 'tau-prompt-1',
      'tau.runtime.id': 'runtime-1',
      'pi.generation': 1,
    });
    expect(checkpoint?.traceId).toBe(rpc.context?.traceId);
    expect(checkpoint?.spanId).toBe(rpc.context?.spanId);
    expect(records.some((record) => record.family === 'pi.rpc')).toBe(false);
  });

  it('does not checkpoint an ordinary tauri.invoke span', async () => {
    const { startCommandSpan, flushTelemetry } = await loadTelemetry();
    startCommandSpan('load_workspace');

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([name]) => name === 'ingest_telemetry',
    );
    expect(ingestCall).toBeUndefined();
  });
});

describe('recordStreamAggregate', () => {
  it('records exactly one bounded aggregate span per call, never one per delta', async () => {
    const { recordStreamAggregate, flushTelemetry } = await loadTelemetry();
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
  traceId?: string;
  spanId?: string;
  parentSpanId?: string;
}

function flushedRecords(): IngestedRecord[] {
  return mockInvoke.mock.calls
    .filter(([name]) => name === 'ingest_telemetry')
    .flatMap(([, args]) => (args as { records: IngestedRecord[] }).records);
}

describe('recordRpcResponseAnomaly', () => {
  it('records a bounded request id and runtime scope without response content', async () => {
    const { recordRpcResponseAnomaly, flushTelemetry } = await loadTelemetry();
    recordRpcResponseAnomaly('unmatched_or_duplicate', 'tau-state-1', {
      sessionId: 'session-1',
      runtimeId: 'runtime-1',
      generation: 2,
    });

    await flushTelemetry();

    const anomaly = flushedRecords().find(
      (record) => record.family === 'pi.rpc.anomaly',
    );
    expect(anomaly?.attributes).toEqual({
      'pi.rpc.anomaly.kind': 'unmatched_or_duplicate',
      'pi.rpc.request_id': 'tau-state-1',
      'tau.session.id': 'session-1',
      'tau.runtime.id': 'runtime-1',
      'pi.generation': 2,
    });
  });
});

describe('recordControllerTransition', () => {
  it('records state before/after, cause, and session/controller/runtime context', async () => {
    const { recordControllerTransition, flushTelemetry } =
      await loadTelemetry();
    recordControllerTransition('idle', 'starting', 'controller_start', {
      sessionId: 'session-1',
      controllerId: 'controller-1',
      runtimeId: 'runtime-1',
      generation: 2,
    });

    await flushTelemetry();

    const transition = flushedRecords().find(
      (record) => record.family === 'controller.lifecycle',
    );
    expect(transition?.attributes).toEqual({
      'tau.controller.state.before': 'idle',
      'tau.controller.state.after': 'starting',
      'tau.controller.transition.cause': 'controller_start',
      'tau.session.id': 'session-1',
      'tau.controller.id': 'controller-1',
      'tau.runtime.id': 'runtime-1',
      'pi.generation': 2,
    });
  });

  it('carries the active span context when given one', async () => {
    const { recordControllerTransition, startActionSpan, flushTelemetry } =
      await loadTelemetry();
    const action = startActionSpan('message.send');

    recordControllerTransition(
      'idle',
      'working',
      'message_send',
      { controllerId: 'controller-1' },
      action.context,
    );

    await flushTelemetry();

    const transition = flushedRecords().find(
      (record) => record.family === 'controller.lifecycle',
    );
    expect(transition?.traceId).toBe(action.context?.traceId);
    expect(transition?.spanId).toBe(action.context?.spanId);
  });
});

describe('recordEventLoopLag', () => {
  it('records a bounded lag value with visibility/focus dimensions', async () => {
    const { recordEventLoopLag, flushTelemetry } = await loadTelemetry();
    recordEventLoopLag(42, 'visible', true);

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'frontend.event_loop_lag',
            value: 42,
            attributes: {
              'tau.heartbeat.visibility': 'visible',
              'tau.heartbeat.focused': 'true',
            },
          }),
        ],
      }),
    );
  });

  it('drops a negative, non-finite, or out-of-bounds value rather than sending it', async () => {
    const { recordEventLoopLag, flushTelemetry } = await loadTelemetry();
    const { MAX_METRIC_VALUE_MS } = await import('./metric');
    recordEventLoopLag(-1, 'visible', true);
    recordEventLoopLag(Number.POSITIVE_INFINITY, 'visible', true);
    recordEventLoopLag(Number.NaN, 'visible', true);
    recordEventLoopLag(MAX_METRIC_VALUE_MS + 1, 'visible', true);

    await flushTelemetry();

    expect(
      flushedRecords().some(
        (record) => record.family === 'frontend.event_loop_lag',
      ),
    ).toBe(false);
  });
});

describe('recordLongTask', () => {
  it('records a bounded duration with no attributes', async () => {
    const { recordLongTask, flushTelemetry } = await loadTelemetry();
    recordLongTask(75);

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'frontend.long_task',
            value: 75,
            attributes: {},
          }),
        ],
      }),
    );
  });
});

describe('recordHeartbeat', () => {
  it('records visibility, focus, and coarse counts', async () => {
    const { recordHeartbeat, flushTelemetry } = await loadTelemetry();
    recordHeartbeat({
      visibility: 'hidden',
      focused: false,
      pendingRpcCount: 2,
      controllerCount: 3,
      activeControllerCount: 1,
      runtimeCount: 2,
      queueLength: 4,
    });

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'frontend.heartbeat',
            attributes: {
              'tau.heartbeat.visibility': 'hidden',
              'tau.heartbeat.focused': 'false',
              'tau.heartbeat.pending_rpc_count': 2,
              'tau.heartbeat.controller_count': 3,
              'tau.heartbeat.active_controller_count': 1,
              'tau.heartbeat.runtime_count': 2,
              'tau.heartbeat.queue_length': 4,
            },
          }),
        ],
      }),
    );
  });
});

describe('recordStateSummary', () => {
  it('records counts and a draft bucket, never draft or transcript text', async () => {
    const { recordStateSummary, flushTelemetry } = await loadTelemetry();
    recordStateSummary({
      controllerCount: 2,
      runtimeCount: 1,
      pendingRpcCount: 0,
      notificationCount: 1,
      dialogCount: 0,
      transcriptCounts: {
        user: 3,
        assistant: 3,
        tool: 1,
        thinking: 2,
        error: 0,
      },
      draftBucket: 'short',
      oldestPendingRpcAgeMs: 0,
      scope: { sessionId: 'session-1' },
    });

    await flushTelemetry();

    expect(mockInvoke).toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.objectContaining({
        records: [
          expect.objectContaining({
            family: 'frontend.state_summary',
            attributes: {
              'tau.state.controller_count': 2,
              'tau.state.runtime_count': 1,
              'tau.state.pending_rpc_count': 0,
              'tau.state.notification_count': 1,
              'tau.state.dialog_count': 0,
              'tau.state.transcript.user_count': 3,
              'tau.state.transcript.assistant_count': 3,
              'tau.state.transcript.tool_count': 1,
              'tau.state.transcript.thinking_count': 2,
              'tau.state.transcript.error_count': 0,
              'tau.state.draft_bucket': 'short',
              'tau.state.oldest_pending_rpc_age_ms': 0,
              'tau.session.id': 'session-1',
            },
          }),
        ],
      }),
    );
  });
});

describe('vueErrorHandler', () => {
  it('records the bounded error kind and a sanitized location, never the message', async () => {
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
    const { vueErrorHandler, flushTelemetry } = await loadTelemetry();
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
    const { vueErrorHandler, flushTelemetry } = await loadTelemetry();

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

describe('forbidden-content canary sweep across frontend error capture paths', () => {
  it('never persists any catalog canary as a vueErrorHandler error message', async () => {
    const consoleError = vi
      .spyOn(console, 'error')
      .mockImplementation(() => {});
    const { vueErrorHandler, flushTelemetry } = await loadTelemetry();
    const { FORBIDDEN_CONTENT_CANARIES } = await import('./privacy');

    for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
      vueErrorHandler(new Error(canary));
    }
    await flushTelemetry();

    const records = flushedRecords().filter(
      (record) => record.family === 'frontend.error',
    );
    expect(records).toHaveLength(FORBIDDEN_CONTENT_CANARIES.size);
    const encoded = JSON.stringify(records);
    for (const [name, canary] of FORBIDDEN_CONTENT_CANARIES) {
      expect(encoded, `leaked the ${name} canary`).not.toContain(canary);
    }
    consoleError.mockRestore();
  });

  it('never persists any catalog canary through the wrapped console.error', async () => {
    console.error = vi.fn();
    const { installFrontendErrorCapture, flushTelemetry } =
      await loadTelemetry();
    const { FORBIDDEN_CONTENT_CANARIES } = await import('./privacy');
    installFrontendErrorCapture();

    for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
      console.error(new Error(canary));
    }
    await flushTelemetry();

    const records = flushedRecords().filter(
      (record) => record.family === 'frontend.error',
    );
    expect(records).toHaveLength(FORBIDDEN_CONTENT_CANARIES.size);
    const encoded = JSON.stringify(records);
    for (const [name, canary] of FORBIDDEN_CONTENT_CANARIES) {
      expect(encoded, `leaked the ${name} canary`).not.toContain(canary);
    }
  });
});

describe('installFrontendErrorCapture', () => {
  it('wraps console.error, still calling the original, and records a sanitized entry', async () => {
    const nativeConsoleError = vi.fn();

    console.error = nativeConsoleError;
    const { installFrontendErrorCapture, flushTelemetry } =
      await loadTelemetry();
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
      await loadTelemetry();
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
    const { startCommandSpan, flushTelemetry } = await loadTelemetry();

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
    const { startCommandSpan, flushTelemetry } = await loadTelemetry();

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
