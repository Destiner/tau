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
  CONTROLLER_LIFECYCLE,
  FRONTEND_ERROR,
  FRONTEND_EVENT_LOOP_LAG,
  FRONTEND_HEARTBEAT,
  FRONTEND_LONG_TASK,
  FRONTEND_STATE_SUMMARY,
  OPERATION_CHECKPOINT,
  PI_RPC,
  PI_RPC_ANOMALY,
  PI_STREAM,
  TAURI_INVOKE,
  TELEMETRY_HEALTH,
  UI_ACTION,
  validateAttribute,
  type ControllerLifecycleCause,
  type ControllerLifecycleState,
  type DraftLengthBucket,
  type FrontendErrorSource,
  type HeartbeatVisibility,
  type OperationCheckpointFamily,
  type PiRpcAnomalyKind,
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
import { MAX_METRIC_VALUE_MS, type FrontendMetricRecord } from './metric';
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
/** Telemetry records nothing until admin mode says it may (see
 * `src/lib/admin-mode.ts`). Off is the starting state, not a fallback: a
 * failed or slow read of the setting leaves the app recording nothing. */
let enabled = false;

/** Turns recording on or off. Every span, log, and metric record already
 * passes through the queue, so gating the queue's `push` is enough to stop
 * all three at once; turning it off also discards whatever was still
 * waiting to be flushed. */
function setTelemetryEnabled(next: boolean): void {
  if (enabled === next) return;
  enabled = next;
  if (next || !state) return;
  state.queue.drain(Number.MAX_SAFE_INTEGER);
}

/** Builds the queue, tracer, and flush scheduler on first use, so a span
 * created before `initTelemetry` runs is still captured rather than lost. */
function ensureState(): AdapterState {
  if (!state) {
    const rawQueue = createBoundedQueue<FrontendQueueRecord>(MAX_QUEUE_SIZE);
    // Dropping at the queue rather than at the flush keeps a disabled
    // adapter from holding records it will never send: nothing recorded
    // while telemetry is off can be flushed by enabling it later.
    const queue: BoundedQueue<FrontendQueueRecord> = {
      push(record) {
        if (!enabled) return;
        rawQueue.push(record);
      },
      drain: (max) => rawQueue.drain(max),
      get length() {
        return rawQueue.length;
      },
      get dropped() {
        return rawQueue.dropped;
      },
    };
    const tracer = createTracer(queue);
    const { scheduleFlush: rawScheduleFlush, flushNow } =
      createFlushScheduler(queue);
    // Every call site below already calls `scheduleFlush` once its span or
    // log is queued, so piggybacking the overflow check here reports a new
    // drop promptly without threading a callback through `tracer.ts`.
    function scheduleFlush(): void {
      if (!enabled) return;
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

function logScopeAttributes(
  family: string,
  scope: TelemetryScope | undefined,
): Record<string, string | number> {
  const attributes: Record<string, string | number> = {};
  if (!scope) return attributes;
  const candidates: ReadonlyArray<[string, string | number | undefined]> = [
    ['tau.session.id', scope.sessionId],
    ['tau.controller.id', scope.controllerId],
    ['tau.runtime.id', scope.runtimeId],
    ['pi.generation', scope.generation],
  ];
  for (const [key, value] of candidates) {
    if (value === undefined || value === '') continue;
    if (validateAttribute(family, key, value).valid) attributes[key] = value;
  }
  return attributes;
}

/**
 * Records one linked `operation.checkpoint` log the moment `context`'s span
 * starts, so the operation it names stays visible in the persisted timeline
 * even if that span never ends — a hang, a crash, or an abandoned request
 * all leave an OTel span exporter nothing to write. Carries the span's own
 * `traceId`/`spanId`, never a parent's, so the checkpoint links to exactly
 * the operation it is standing in for.
 */
function recordOperationCheckpoint(
  operationFamily: OperationCheckpointFamily,
  operationName: string,
  context: TraceContext,
  scope?: TelemetryScope,
  requestId?: string,
): void {
  try {
    const { queue, scheduleFlush, flushNow } = ensureState();
    const family = OPERATION_CHECKPOINT.name;
    const record: FrontendLogRecord = {
      family,
      timeUnixNano: nowUnixNanoString(),
      attributes: logScopeAttributes(family, scope),
      traceId: context.traceId,
      spanId: context.spanId,
    };
    const fields: ReadonlyArray<[string, string]> = [
      ['tau.operation.family', operationFamily],
      ['tau.operation.name', operationName],
    ];
    for (const [key, value] of fields) {
      if (validateAttribute(family, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    if (
      requestId &&
      validateAttribute(family, 'pi.rpc.request_id', requestId).valid
    ) {
      record.attributes['pi.rpc.request_id'] = requestId;
    }
    queue.push(record);
    scheduleFlush();
    void flushNow();
  } catch {
    // Telemetry completion must not affect the operation it is checkpointing.
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
    const linkedContext: TraceContext = {
      traceId: spanContext.traceId,
      spanId: spanContext.spanId,
      sampled: (spanContext.traceFlags & TraceFlags.SAMPLED) !== 0,
    };
    if (family === UI_ACTION.name) {
      recordOperationCheckpoint(
        'ui.action',
        attributeValue,
        linkedContext,
        scope,
      );
    }

    return {
      context: linkedContext,
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
    const linkedContext: TraceContext = {
      traceId: spanContext.traceId,
      spanId: spanContext.spanId,
      sampled: (spanContext.traceFlags & TraceFlags.SAMPLED) !== 0,
    };
    recordOperationCheckpoint(
      'pi.rpc',
      method,
      linkedContext,
      {
        ...scope,
        runtimeId,
        generation,
      },
      requestId,
    );

    return {
      context: linkedContext,
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

function recordRpcResponseAnomaly(
  kind: PiRpcAnomalyKind,
  requestId: string,
  scope?: TelemetryScope,
): void {
  try {
    const { queue, scheduleFlush, flushNow } = ensureState();
    const family = PI_RPC_ANOMALY.name;
    const record: FrontendLogRecord = {
      family,
      timeUnixNano: nowUnixNanoString(),
      attributes: logScopeAttributes(family, scope),
    };
    const fields: ReadonlyArray<[string, string]> = [
      ['pi.rpc.anomaly.kind', kind],
      ['pi.rpc.request_id', requestId],
    ];
    for (const [key, value] of fields) {
      if (validateAttribute(family, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    queue.push(record);
    scheduleFlush();
    void flushNow();
  } catch {
    // Telemetry failure must not affect response handling.
  }
}

/**
 * Records one `controller.lifecycle` transition: a named composite-state
 * change with its cause, plus session/controller/runtime context and, when
 * the mutation happened inside an active span, that span's own trace
 * context. Called only from `composables/state.ts`'s
 * `setControllerLifecycle`, never at every raw boolean assignment site —
 * see that function for why.
 */
function recordControllerTransition(
  before: ControllerLifecycleState,
  after: ControllerLifecycleState,
  cause: ControllerLifecycleCause,
  scope?: TelemetryScope,
  context?: TraceContext,
): void {
  try {
    const { queue, scheduleFlush } = ensureState();
    const family = CONTROLLER_LIFECYCLE.name;
    const record: FrontendLogRecord = {
      family,
      timeUnixNano: nowUnixNanoString(),
      attributes: logScopeAttributes(family, scope),
    };
    const fields: ReadonlyArray<[string, string]> = [
      ['tau.controller.state.before', before],
      ['tau.controller.state.after', after],
      ['tau.controller.transition.cause', cause],
    ];
    for (const [key, value] of fields) {
      if (validateAttribute(family, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    if (context) {
      record.traceId = context.traceId;
      record.spanId = context.spanId;
    }
    queue.push(record);
    scheduleFlush();
  } catch {
    // Telemetry completion must not affect controller lifecycle handling.
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

/** The bounded queue's current length: how many spans/logs/metric records
 * are waiting for the next flush. Read only by `./heartbeat` for the
 * periodic heartbeat's `tau.heartbeat.queue_length` gauge — never exposes
 * the queue itself. */
function telemetryQueueLength(): number {
  try {
    return ensureState().queue.length;
  } catch {
    return 0;
  }
}

interface HeartbeatInput {
  visibility: HeartbeatVisibility;
  focused: boolean;
  pendingRpcCount: number;
  controllerCount: number;
  activeControllerCount: number;
  runtimeCount: number;
  queueLength: number;
}

/**
 * Records one low-frequency `frontend.heartbeat` log: liveness plus
 * visibility/focus state and coarse, content-free counts. Called on a timer
 * from `./heartbeat`, never from product code directly. A gap between
 * heartbeats — or the absence of the next one — is itself the diagnostic
 * signal for a slow or stuck frontend; visibility/focus travel alongside so
 * a gap while hidden (background timer throttling) is not mistaken for one
 * while the window was actually active.
 */
function recordHeartbeat(input: HeartbeatInput): void {
  try {
    const { queue, scheduleFlush } = ensureState();
    const family = FRONTEND_HEARTBEAT.name;
    const record: FrontendLogRecord = {
      family,
      timeUnixNano: nowUnixNanoString(),
      attributes: {},
    };
    const fields: ReadonlyArray<[string, string | number]> = [
      ['tau.heartbeat.visibility', input.visibility],
      ['tau.heartbeat.focused', input.focused ? 'true' : 'false'],
      ['tau.heartbeat.pending_rpc_count', input.pendingRpcCount],
      ['tau.heartbeat.controller_count', input.controllerCount],
      ['tau.heartbeat.active_controller_count', input.activeControllerCount],
      ['tau.heartbeat.runtime_count', input.runtimeCount],
      ['tau.heartbeat.queue_length', input.queueLength],
    ];
    for (const [key, value] of fields) {
      if (validateAttribute(family, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    queue.push(record);
    scheduleFlush();
  } catch {
    // Telemetry completion must not affect the heartbeat timer.
  }
}

/**
 * Shared implementation behind `recordEventLoopLag`/`recordLongTask`: the
 * two raw measurements with no native span/log counterpart to derive a
 * metric from, so they cross IPC as a dedicated `FrontendMetricRecord`
 * instead of being derived natively the way every other Stage 5 metric is.
 * `value` is hard-bounded and must be a finite, non-negative number; every
 * attribute is still revalidated against the family's own catalog entry
 * before being attached.
 */
function recordMetric(
  family: string,
  value: number,
  attributes: Record<string, string | number>,
): void {
  try {
    if (!Number.isFinite(value) || value < 0 || value > MAX_METRIC_VALUE_MS) {
      return;
    }
    const { queue, scheduleFlush } = ensureState();
    const record: FrontendMetricRecord = {
      family,
      value,
      timeUnixNano: nowUnixNanoString(),
      attributes: {},
    };
    for (const [key, attributeValue] of Object.entries(attributes)) {
      if (validateAttribute(family, key, attributeValue).valid) {
        record.attributes[key] = attributeValue;
      }
    }
    queue.push(record);
    scheduleFlush();
  } catch {
    // Telemetry completion must not affect whatever is being measured.
  }
}

/**
 * Records one raw `frontend.event_loop_lag` measurement (scheduled versus
 * actual delay between heartbeat ticks) as a native OTel histogram.
 * `visibility`/`focused` travel as bounded dimensions so a large reading
 * while hidden (expected background-timer throttling) is distinguishable
 * from one while the window was active, without this function itself
 * deciding what counts as a failure.
 */
function recordEventLoopLag(
  lagMs: number,
  visibility: HeartbeatVisibility,
  focused: boolean,
): void {
  recordMetric(FRONTEND_EVENT_LOOP_LAG.name, lagMs, {
    'tau.heartbeat.visibility': visibility,
    'tau.heartbeat.focused': focused ? 'true' : 'false',
  });
}

/** Records one `PerformanceObserver` `longtask` entry's duration as a
 * native OTel histogram. No dimensions: a long task carries no reviewed
 * attribution. */
function recordLongTask(durationMs: number): void {
  recordMetric(FRONTEND_LONG_TASK.name, durationMs, {});
}

interface TranscriptKindCounts {
  user: number;
  assistant: number;
  tool: number;
  thinking: number;
  error: number;
}

interface StateSummaryInput {
  controllerCount: number;
  runtimeCount: number;
  pendingRpcCount: number;
  notificationCount: number;
  dialogCount: number;
  transcriptCounts: TranscriptKindCounts;
  draftBucket: DraftLengthBucket;
  oldestPendingRpcAgeMs: number;
  scope?: TelemetryScope;
}

/**
 * Records one periodic `frontend.state_summary` log: shape and counts only,
 * never transcript or draft text. Called on a timer from `./heartbeat`.
 * Active project/session/controller identifiers, when `scope` names them,
 * are attached as ordinary context attributes — the same mechanism spans
 * and other logs already use — not new family-specific attributes.
 */
function recordStateSummary(input: StateSummaryInput): void {
  try {
    const { queue, scheduleFlush } = ensureState();
    const family = FRONTEND_STATE_SUMMARY.name;
    const record: FrontendLogRecord = {
      family,
      timeUnixNano: nowUnixNanoString(),
      attributes: logScopeAttributes(family, input.scope),
    };
    const fields: ReadonlyArray<[string, string | number]> = [
      ['tau.state.controller_count', input.controllerCount],
      ['tau.state.runtime_count', input.runtimeCount],
      ['tau.state.pending_rpc_count', input.pendingRpcCount],
      ['tau.state.notification_count', input.notificationCount],
      ['tau.state.dialog_count', input.dialogCount],
      ['tau.state.transcript.user_count', input.transcriptCounts.user],
      [
        'tau.state.transcript.assistant_count',
        input.transcriptCounts.assistant,
      ],
      ['tau.state.transcript.tool_count', input.transcriptCounts.tool],
      ['tau.state.transcript.thinking_count', input.transcriptCounts.thinking],
      ['tau.state.transcript.error_count', input.transcriptCounts.error],
      ['tau.state.draft_bucket', input.draftBucket],
      ['tau.state.oldest_pending_rpc_age_ms', input.oldestPendingRpcAgeMs],
    ];
    for (const [key, value] of fields) {
      if (validateAttribute(family, key, value).valid) {
        record.attributes[key] = value;
      }
    }
    queue.push(record);
    scheduleFlush();
  } catch {
    // Telemetry completion must not affect the state-summary timer.
  }
}

export type {
  CommandSpanHandle,
  RpcSpanHandle,
  StateSummaryInput,
  TelemetryScope,
};

export {
  flushTelemetry,
  initTelemetry,
  installFrontendErrorCapture,
  invokeTraced,
  recordControllerTransition,
  recordEventLoopLag,
  recordHeartbeat,
  recordLongTask,
  recordRpcResponseAnomaly,
  recordStateSummary,
  recordStreamAggregate,
  setTelemetryEnabled,
  startActionSpan,
  startCommandSpan,
  startRpcSpan,
  telemetryQueueLength,
  vueErrorHandler,
};
