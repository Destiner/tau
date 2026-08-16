/*
 * Tau's frontend telemetry adapter: the only surface product code should
 * use. It owns the bounded queue, the tracer, and the flush scheduler, so
 * nothing else in the app imports `@opentelemetry/api`/`sdk-trace` or the
 * Tauri ingest command directly.
 */
import {
  ROOT_CONTEXT,
  TraceFlags,
  type Span,
  type Tracer,
} from '@opentelemetry/api';
import { invoke } from '@tauri-apps/api/core';

import {
  PI_RPC,
  PI_STREAM,
  TAURI_INVOKE,
  UI_ACTION,
  validateAttribute,
  type PiRpcMethod,
  type PiRpcOutcome,
  type TauriInvokeCommand,
  type UiActionName,
} from './attributes';
import { createFlushScheduler } from './ingest';
import { createBoundedQueue, type BoundedQueue } from './queue';
import type { TraceContext } from './trace-context';
import {
  createTracer,
  remoteParentContext,
  type FrontendSpanRecord,
} from './tracer';

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

interface TelemetryScope {
  sessionId?: string;
  controllerId?: string;
  runtimeId?: string;
  generation?: number;
}

interface CommandSpanHandle {
  /** Passed to the corresponding Tauri command as `telemetryContext`, so the
   * native span it starts becomes a child of this one. */
  readonly context?: TraceContext;
  end: () => void;
}

/** Starts a span in `family`, carrying one attribute, as a child of
 * `parentContext` when given and a root span otherwise. `parentContext` is
 * always an explicit argument, never read from ambient/global state, so
 * concurrent action → invoke chains across sessions and controllers cannot
 * leak context into each other. Shared by `startActionSpan` and
 * `startCommandSpan`; Pi RPC spans use `startRpcSpan` instead, since they
 * need more than one attribute and end asynchronously. */
function setScopeAttributes(
  span: Span,
  family: string,
  scope: TelemetryScope | undefined,
): void {
  if (!scope) return;
  const attributes: ReadonlyArray<[string, string | number | undefined]> = [
    ['tau.session.id', scope.sessionId],
    ['tau.controller.id', scope.controllerId],
    ['tau.runtime.id', scope.runtimeId],
    ['pi.generation', scope.generation],
  ];
  for (const [key, value] of attributes) {
    if (value === undefined || value === '') continue;
    if (validateAttribute(family, key, value).valid)
      span.setAttribute(key, value);
  }
}

function startFamilySpan(
  family: string,
  attributeKey: string,
  attributeValue: string,
  parentContext?: TraceContext,
  scope?: TelemetryScope,
): CommandSpanHandle {
  try {
    const { tracer, scheduleFlush } = ensureState();
    const context = parentContext
      ? remoteParentContext(parentContext)
      : ROOT_CONTEXT;
    const span = tracer.startSpan(family, undefined, context);
    if (validateAttribute(family, attributeKey, attributeValue).valid) {
      span.setAttribute(attributeKey, attributeValue);
    }
    setScopeAttributes(span, family, scope);
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

/**
 * Starts a `ui.action` root span for a semantic user action (session
 * selection, message send, stop, rename, model/effort selection, extension
 * dialog responses). `useTau.ts` passes the returned context down through
 * the invoke wrapper and Pi RPC calls the action makes, so a reconstructed
 * trace shows the whole action → invoke → RPC timeline.
 */
function startActionSpan(
  action: UiActionName,
  scope?: TelemetryScope,
): CommandSpanHandle {
  return startFamilySpan(
    UI_ACTION.name,
    'tau.action.name',
    action,
    undefined,
    scope,
  );
}

/**
 * Starts a `tauri.invoke` span for one ordinary Tauri command call, either
 * as a root span or as a child of an enclosing action's context.
 */
function startCommandSpan(
  command: TauriInvokeCommand,
  parentContext?: TraceContext,
): CommandSpanHandle {
  return startFamilySpan(
    TAURI_INVOKE.name,
    'tau.invoke.command',
    command,
    parentContext,
  );
}

/**
 * The one Tau-owned wrapper for ordinary Tauri invokes: starts a
 * `tauri.invoke` span, passes its context to the command as
 * `telemetryContext`, and ends the span once the call settles either way.
 * `command` is drawn from the reviewed `TauriInvokeCommand` allowlist, which
 * deliberately excludes `send_pi` (covered by Pi RPC spans instead) and
 * `ingest_telemetry` (which must never trace itself). `args` is only ever
 * used to make the real invoke call — it is never read into telemetry, so
 * this wrapper never serializes a command's arguments.
 *
 * Deliberately not an `async function`: returning the exact promise
 * `invoke` produces (rather than a promise from an `await`-wrapping
 * function body) keeps callers' continuations exactly as many microtask
 * ticks away from resolution as an unwrapped `invoke` call, so telemetry
 * never shifts the relative ordering product code or tests observe.
 * Ending the span is attached as an independent `.then()` on that same
 * promise rather than chained into the return value, for the same reason.
 */
function invokeTraced<T>(
  command: TauriInvokeCommand,
  args?: Record<string, unknown>,
  parentContext?: TraceContext,
): Promise<T> {
  const span = startCommandSpan(command, parentContext);
  let promise: Promise<T>;
  try {
    promise = invoke<T>(command, { ...args, telemetryContext: span.context });
  } catch (error) {
    span.end();
    return Promise.reject(error);
  }
  promise.then(
    () => span.end(),
    () => span.end(),
  );
  return promise;
}

interface RpcSpanHandle {
  readonly context?: TraceContext;
  /** Ends the span with the given outcome. Idempotent from the caller's
   * perspective: `runtime.ts` never calls this twice for the same pending
   * span, since it removes the span from its pending map first. */
  end: (outcome: PiRpcOutcome) => void;
}

/**
 * Starts a `pi.rpc` span at the moment Tau creates a Pi RPC request (before
 * `send_pi` even writes it), carrying the method, request id, and the
 * runtime/generation the request belongs to. `runtime.ts` keeps the
 * returned handle in a pending map keyed by runtime + generation + request
 * id and calls `end` on the exact matching response, a timeout, or explicit
 * abandonment (process exit, generation change, controller disposal or
 * replacement, stop paths) — never through ambient state.
 */
function startRpcSpan(
  method: PiRpcMethod,
  requestId: string,
  runtimeId: string,
  generation: number,
  parentContext?: TraceContext,
  scope?: Pick<TelemetryScope, 'sessionId' | 'controllerId'>,
): RpcSpanHandle {
  try {
    const { tracer, scheduleFlush } = ensureState();
    const context = parentContext
      ? remoteParentContext(parentContext)
      : ROOT_CONTEXT;
    const span = tracer.startSpan(PI_RPC.name, undefined, context);
    const attributes: ReadonlyArray<[string, string | number]> = [
      ['pi.rpc.method', method],
      ['pi.rpc.request_id', requestId],
      ['tau.runtime.id', runtimeId],
      ['pi.generation', generation],
    ];
    for (const [key, value] of attributes) {
      if (validateAttribute(PI_RPC.name, key, value).valid) {
        span.setAttribute(key, value);
      }
    }
    setScopeAttributes(span, PI_RPC.name, scope);
    const spanContext = span.spanContext();

    return {
      context: {
        traceId: spanContext.traceId,
        spanId: spanContext.spanId,
        sampled: (spanContext.traceFlags & TraceFlags.SAMPLED) !== 0,
      },
      end(outcome: PiRpcOutcome): void {
        try {
          if (validateAttribute(PI_RPC.name, 'pi.rpc.outcome', outcome).valid) {
            span.setAttribute('pi.rpc.outcome', outcome);
          }
          span.end();
          scheduleFlush();
        } catch {
          // Telemetry completion must not affect Pi RPC handling.
        }
      },
    };
  } catch {
    return { end(): void {} };
  }
}

/**
 * Records one `pi.stream` aggregate span for a Pi run's streaming deltas:
 * a bounded count and total character count, never one record per delta or
 * token. `startTime`/`endTime` are epoch milliseconds spanning the run's
 * first delta through the moment `runtime.ts` flushes the aggregate (settle,
 * abandonment, or stop).
 */
function recordStreamAggregate(
  runtimeId: string,
  generation: number,
  deltaCount: number,
  characterCount: number,
  startTime: number,
  endTime: number,
  scope?: Pick<TelemetryScope, 'sessionId' | 'controllerId'>,
): void {
  try {
    const { tracer, scheduleFlush } = ensureState();
    const span = tracer.startSpan(PI_STREAM.name, { startTime }, ROOT_CONTEXT);
    const attributes: ReadonlyArray<[string, string | number]> = [
      ['pi.stream.delta_count', deltaCount],
      ['pi.stream.character_count', characterCount],
      ['tau.runtime.id', runtimeId],
      ['pi.generation', generation],
    ];
    for (const [key, value] of attributes) {
      if (validateAttribute(PI_STREAM.name, key, value).valid) {
        span.setAttribute(key, value);
      }
    }
    setScopeAttributes(span, PI_STREAM.name, scope);
    span.end(endTime);
    scheduleFlush();
  } catch {
    // Telemetry completion must not affect streaming.
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

export type { CommandSpanHandle, RpcSpanHandle, TelemetryScope };

export {
  flushTelemetry,
  initTelemetry,
  invokeTraced,
  recordStreamAggregate,
  startActionSpan,
  startCommandSpan,
  startRpcSpan,
};
