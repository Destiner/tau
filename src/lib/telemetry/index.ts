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
  FRONTEND_ERROR,
  PI_RPC,
  PI_STREAM,
  TAURI_INVOKE,
  TELEMETRY_HEALTH,
  UI_ACTION,
  validateAttribute,
  type FrontendErrorSource,
  type PiRpcMethod,
  type PiRpcOutcome,
  type TauriInvokeCommand,
  type TauriInvokeOutcome,
  type UiActionName,
} from './attributes';
import {
  classifyErrorKind,
  locationFromErrorEvent,
  locationFromValue,
} from './errors';
import { createFlushScheduler } from './ingest';
import { nowUnixNanoString, type FrontendLogRecord } from './log';
import { createBoundedQueue, type BoundedQueue } from './queue';
import type { TraceContext } from './trace-context';
import {
  createTracer,
  remoteParentContext,
  type FrontendQueueRecord,
} from './tracer';

/** Bounds both the pre-init and batch buffer; see `./queue`. */
const MAX_QUEUE_SIZE = 200;

interface AdapterState {
  queue: BoundedQueue<FrontendQueueRecord>;
  tracer: Tracer;
  scheduleFlush: () => void;
  flushNow: () => Promise<void>;
}

let state: AdapterState | undefined;
/** The queue's own `dropped` count as of the last time it was reported via
 * a `telemetry.health` log, so overflow is reported once per new drop
 * rather than on every flush. */
let lastReportedDroppedCount = 0;

/** Builds the queue, tracer, and flush scheduler on first use, so a span
 * created before `initTelemetry` runs is still captured rather than lost. */
function ensureState(): AdapterState {
  if (!state) {
    const queue = createBoundedQueue<FrontendQueueRecord>(MAX_QUEUE_SIZE);
    const tracer = createTracer(queue);
    const { scheduleFlush: rawScheduleFlush, flushNow } =
      createFlushScheduler(queue);
    // Every call site below already calls `scheduleFlush` once its span or
    // log is queued, so piggybacking the overflow check here reports a new
    // drop promptly without threading a callback through `tracer.ts`.
    function scheduleFlush(): void {
      reportQueueOverflowIfChanged(queue);
      rawScheduleFlush();
    }
    state = { queue, tracer, scheduleFlush, flushNow };
  }
  return state;
}

/** Queues one `telemetry.health` log reporting the queue's current
 * `dropped` count, but only once per new drop — not on every flush — so
 * reporting overflow cannot itself contribute to more of it. Pushed
 * directly into the queue it is reporting on, never through a second
 * `ensureState()`/tracer round trip. */
function reportQueueOverflowIfChanged(
  queue: BoundedQueue<FrontendQueueRecord>,
): void {
  if (queue.dropped === lastReportedDroppedCount) return;
  const droppedCount = queue.dropped;
  const record: FrontendLogRecord = {
    family: TELEMETRY_HEALTH.name,
    timeUnixNano: nowUnixNanoString(),
    attributes: {},
  };
  if (
    validateAttribute(
      TELEMETRY_HEALTH.name,
      'tau.telemetry.dropped_count',
      droppedCount,
    ).valid
  ) {
    record.attributes['tau.telemetry.dropped_count'] = droppedCount;
  }
  queue.push(record);
  // Re-read after pushing: if the queue was already at capacity, this push
  // may have evicted another record and incremented `dropped` again. Using
  // the post-push value here means that increment is never mistaken for a
  // fresh drop on a later, unrelated call.
  lastReportedDroppedCount = queue.dropped;
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
  end: (outcome?: TauriInvokeOutcome) => void;
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
      end(outcome?: TauriInvokeOutcome): void {
        try {
          if (
            family === TAURI_INVOKE.name &&
            outcome !== undefined &&
            validateAttribute(family, 'tau.invoke.outcome', outcome).valid
          ) {
            span.setAttribute('tau.invoke.outcome', outcome);
          }
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
    span.end('error');
    return Promise.reject(error);
  }
  promise.then(
    () => span.end('success'),
    () => span.end('error'),
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

/**
 * Records one `frontend.error` log for a captured `window.error`,
 * `unhandledrejection`, Vue error, or `console.error` call. Never the
 * thrown value's message or a serialized object — only its bounded
 * `kind`/`source` category and a sanitized source location, each dropped
 * (not persisted) individually if it somehow fails validation, so a
 * classification bug can never smuggle an unreviewed value through.
 */
let inConsoleErrorCapture = false;
let frontendErrorCaptureInstalled = false;

function recordFrontendError(
  source: FrontendErrorSource,
  kind: string,
  location: string,
): void {
  try {
    const { queue, scheduleFlush, flushNow } = ensureState();
    const record: FrontendLogRecord = {
      family: FRONTEND_ERROR.name,
      timeUnixNano: nowUnixNanoString(),
      attributes: {},
    };
    const candidates: ReadonlyArray<[string, string]> = [
      ['tau.error.source', source],
      ['tau.error.kind', kind],
      ['tau.error.location', location],
    ];
    for (const [key, value] of candidates) {
      if (validateAttribute(FRONTEND_ERROR.name, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    queue.push(record);
    scheduleFlush();
    void flushNow();
  } catch {
    // Telemetry completion must not affect error handling itself.
  }
}

/** Assigned to `app.config.errorHandler`. Vue calls this synchronously with
 * whatever was thrown; only its bounded category and sanitized location
 * are ever recorded, never `err` itself or Vue's own `info` string (which
 * is not part of the reviewed catalog). */
function vueErrorHandler(err: unknown): void {
  recordFrontendError(
    'vue_error',
    classifyErrorKind(err),
    locationFromValue(err),
  );
  const wasCapturing = inConsoleErrorCapture;
  inConsoleErrorCapture = true;
  try {
    console.error(err);
  } catch {
    // Preserve telemetry's fail-open behavior even if the console is patched.
  } finally {
    inConsoleErrorCapture = wasCapturing;
  }
}

/** Guards `console.error` wrapping against recording its own re-entry: if
 * sanitizing/recording somehow calls `console.error` again on the same
 * thread (a bug in this module, not expected), the nested call is still
 * forwarded to the real `console.error` but is not itself recorded. */

/**
 * Installs `window.error`/`unhandledrejection` listeners and wraps
 * `console.error`. Each installation is independently guarded so a
 * DOM-less environment (a unit test) can still exercise the parts that do
 * not need `window`. Safe to call once at startup, before Vue mounts:
 * every listener/wrapper here is synchronous and does no I/O, so nothing
 * delays window reveal or mounting.
 */
function installFrontendErrorCapture(): void {
  if (frontendErrorCaptureInstalled) return;
  frontendErrorCaptureInstalled = true;

  try {
    window.addEventListener('error', (event) => {
      recordFrontendError(
        'window_error',
        classifyErrorKind(event.error),
        locationFromErrorEvent(event),
      );
    });
  } catch {
    // No `window` (e.g. a non-browser test environment): skip this listener.
  }

  try {
    window.addEventListener('unhandledrejection', (event) => {
      recordFrontendError(
        'unhandled_rejection',
        classifyErrorKind(event.reason),
        locationFromValue(event.reason),
      );
    });
  } catch {
    // No `window`: skip this listener too.
  }

  try {
    const original = console.error.bind(console);

    console.error = (...args: unknown[]): void => {
      original(...args);
      if (inConsoleErrorCapture) return;
      inConsoleErrorCapture = true;
      try {
        const first: unknown = args[0];
        recordFrontendError(
          'console_error',
          classifyErrorKind(first),
          locationFromValue(first),
        );
      } finally {
        inConsoleErrorCapture = false;
      }
    };
  } catch {
    // Telemetry must never prevent console.error from working normally.
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
  installFrontendErrorCapture,
  invokeTraced,
  recordStreamAggregate,
  startActionSpan,
  startCommandSpan,
  startRpcSpan,
  vueErrorHandler,
};
