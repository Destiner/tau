/*
 * Tau's frontend telemetry adapter: the only surface product code should
 * use. It owns the bounded queue, the tracer, and the flush scheduler, so
 * nothing else in the app imports `@opentelemetry/api`/`sdk-trace` or the
 * Tauri ingest command directly.
 */
import { ROOT_CONTEXT, TraceFlags, type Tracer } from '@opentelemetry/api';

import { type TauriInvokeCommand, validateAttribute } from './attributes';
import { createFlushScheduler } from './ingest';
import { createBoundedQueue, type BoundedQueue } from './queue';
import type { TraceContext } from './trace-context';
import { createTracer, type FrontendSpanRecord } from './tracer';

/** Bounds both the pre-init and batch buffer; see `./queue`. */
const MAX_QUEUE_SIZE = 200;

interface AdapterState {
  queue: BoundedQueue<FrontendSpanRecord>;
  tracer: Tracer;
  scheduleFlush: () => void;
  flushNow: () => Promise<void>;
}

let state: AdapterState | undefined;

/** Builds the queue, tracer, and flush scheduler on first use, so a span
 * created before `initTelemetry` runs is still captured rather than lost. */
function ensureState(): AdapterState {
  if (!state) {
    const queue = createBoundedQueue<FrontendSpanRecord>(MAX_QUEUE_SIZE);
    const tracer = createTracer(queue);
    const { scheduleFlush, flushNow } = createFlushScheduler(queue);
    state = { queue, tracer, scheduleFlush, flushNow };
  }
  return state;
}

interface CommandSpanHandle {
  /** Passed to the corresponding Tauri command as `telemetryContext`, so the
   * native span it starts becomes a child of this one. */
  readonly context?: TraceContext;
  end: () => void;
}

/**
 * Starts the one frontend span Stage 2 instruments end to end: the
 * `tauri.invoke` span around a single Tauri command call. Ordinary
 * invoke/action instrumentation is Stage 3's job, not this function's.
 */
function startCommandSpan(command: TauriInvokeCommand): CommandSpanHandle {
  try {
    const { tracer, scheduleFlush } = ensureState();
    const span = tracer.startSpan('tauri.invoke', undefined, ROOT_CONTEXT);
    if (
      validateAttribute('tauri.invoke', 'tau.invoke.command', command).valid
    ) {
      span.setAttribute('tau.invoke.command', command);
    }
    const spanContext = span.spanContext();

    return {
      context: {
        traceId: spanContext.traceId,
        spanId: spanContext.spanId,
        sampled: (spanContext.traceFlags & TraceFlags.SAMPLED) !== 0,
      },
      end(): void {
        try {
          span.end();
          scheduleFlush();
        } catch {
          // Telemetry completion must not affect the wrapped operation.
        }
      },
    };
  } catch {
    return { end(): void {} };
  }
}

/** Constructs the adapter eagerly so a span created immediately at startup
 * is captured from the start. Call before Vue mounts. */
function initTelemetry(): void {
  try {
    ensureState();
  } catch {
    // Telemetry initialization must never prevent the app from mounting.
  }
}

/** Drains any queued spans immediately, bypassing the flush delay. Exposed
 * for callers (and tests) that need queued telemetry sent without waiting. */
function flushTelemetry(): Promise<void> {
  try {
    return ensureState().flushNow();
  } catch {
    return Promise.resolve();
  }
}

export type { CommandSpanHandle };

export { flushTelemetry, initTelemetry, startCommandSpan };
