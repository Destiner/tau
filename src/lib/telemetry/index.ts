import {
  ROOT_CONTEXT,
  TraceFlags,
  type Span,
  type Tracer,
} from '@opentelemetry/api';
import { invoke } from '@tauri-apps/api/core';

import {
  ACTION_MILESTONE,
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
  type ActionMilestoneKind,
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

const MAX_QUEUE_SIZE = 200;

interface AdapterState {
  queue: BoundedQueue<FrontendQueueRecord>;
  tracer: Tracer;
  scheduleFlush: () => void;
  flushNow: () => Promise<void>;
}

let state: AdapterState | undefined;

let lastReportedDroppedCount = 0;
/** Telemetry records nothing until admin mode says it may (see
 * `src/lib/admin-mode.ts`). Off is the starting state, not a fallback: a
 * failed or slow read of the setting leaves the app recording nothing. */
let enabled = false;
let telemetryEpoch = 0;

function setTelemetryEnabled(next: boolean): void {
  if (enabled === next) return;
  enabled = next;
  telemetryEpoch++;
  if (next || !state) return;
  state.queue.drain(Number.MAX_SAFE_INTEGER);
}

function ensureState(): AdapterState {
  if (!state) {
    const rawQueue = createBoundedQueue<FrontendQueueRecord>(MAX_QUEUE_SIZE);

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

    function scheduleFlush(): void {
      if (!enabled) return;
      reportQueueOverflowIfChanged(queue);
      rawScheduleFlush();
    }
    state = { queue, tracer, scheduleFlush, flushNow };
  }
  return state;
}

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

  lastReportedDroppedCount = queue.dropped;
}

interface TelemetryScope {
  sessionId?: string;
  controllerId?: string;
  runtimeId?: string;
  generation?: number;
}

interface CommandSpanHandle {
  readonly context?: TraceContext;
  end: (outcome?: TauriInvokeOutcome) => void;
}

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

// Open spans are not exported after a hang or crash; persist their start separately.
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
    // Diagnostics must not interrupt product work.
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
          // Diagnostics must not interrupt product work.
        }
      },
    };
  } catch {
    return { end(): void {} };
  }
}

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

const PAINT_WAIT_MS = 5_000;

interface ActionMilestones {
  readonly span: CommandSpanHandle;
  mark: (kind: ActionMilestoneKind) => void;

  afterRender: () => void;

  cancel: () => void;
}

/**
 * Starts an action with linked, once-only milestones. `afterRender` should be
 * called after Vue's nextTick following the visible mutation, not when the
 * command resolves. Two animation frames give the browser an opportunity to
 * paint the committed DOM; they cannot prove that pixels reached the screen.
 * Hidden windows and suspended frames get a bounded unavailable marker.
 */
function startActionMilestones(
  action: UiActionName,
  scope?: TelemetryScope,
): ActionMilestones {
  const span = startActionSpan(action, scope);
  const started = performance.now();
  const epoch = telemetryEpoch;
  const seen = new Set<ActionMilestoneKind>();
  let frame: number | undefined;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;
  let paintSettled = false;

  function cancel(): void {
    cancelled = true;
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (timeout !== undefined) clearTimeout(timeout);
  }

  function mark(kind: ActionMilestoneKind): void {
    if (cancelled || !enabled || epoch !== telemetryEpoch || seen.has(kind))
      return;
    seen.add(kind);
    try {
      const elapsed = Math.round(performance.now() - started);
      if (
        !Number.isSafeInteger(elapsed) ||
        elapsed < 0 ||
        elapsed > MAX_METRIC_VALUE_MS
      )
        return;
      const family = ACTION_MILESTONE.name;
      const attributes: Record<string, string | number> = {
        'tau.action.name': action,
        'tau.action.milestone': kind,
        'tau.action.elapsed_ms': elapsed,
      };
      if (
        Object.entries(attributes).some(
          ([key, value]) => !validateAttribute(family, key, value).valid,
        )
      )
        return;
      const { queue, scheduleFlush } = ensureState();
      queue.push({
        family,
        timeUnixNano: nowUnixNanoString(),
        attributes: { ...logScopeAttributes(family, scope), ...attributes },
        ...(span.context && {
          traceId: span.context.traceId,
          spanId: span.context.spanId,
        }),
      });
      scheduleFlush();
    } catch {
      // Diagnostics must not interrupt product work.
    }
  }

  function afterRender(): void {
    if (
      cancelled ||
      !enabled ||
      epoch !== telemetryEpoch ||
      timeout !== undefined ||
      paintSettled
    )
      return;
    try {
      if (document.visibilityState !== 'visible') {
        paintSettled = true;
        mark('paint_unavailable');
        return;
      }
      timeout = setTimeout(() => {
        paintSettled = true;
        if (frame !== undefined) cancelAnimationFrame(frame);
        mark('paint_unavailable');
        timeout = undefined;
      }, PAINT_WAIT_MS);
      frame = requestAnimationFrame(() => {
        if (paintSettled) return;
        frame = requestAnimationFrame(() => {
          if (paintSettled) return;
          paintSettled = true;
          if (timeout !== undefined) clearTimeout(timeout);
          timeout = undefined;
          if (document.visibilityState === 'visible') mark('paint_opportunity');
          else mark('paint_unavailable');
        });
      });
    } catch {
      cancel();
    }
  }

  return { span, mark, afterRender, cancel };
}

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

// Return invoke's original promise: wrapping it in async or chaining the returned
// promise shifts caller continuations relative to uninstrumented invokes.
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
          // Diagnostics must not interrupt product work.
        }
      },
    };
  } catch {
    return { end(): void {} };
  }
}

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
    // Diagnostics must not interrupt product work.
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
    // Diagnostics must not interrupt product work.
  }
}

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
    // Diagnostics must not interrupt product work.
  }
}

let inConsoleErrorCapture = false;
let frontendErrorCaptureInstalled = false;

// Only validated categories and sanitized locations may enter telemetry.
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
    // Diagnostics must not interrupt product work.
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
    // Window may be absent outside the webview.
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
    // Window may be absent outside the webview.
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

function initTelemetry(): void {
  try {
    ensureState();
  } catch {
    // Telemetry initialization must never prevent the app from mounting.
  }
}

function flushTelemetry(): Promise<void> {
  try {
    return ensureState().flushNow();
  } catch {
    return Promise.resolve();
  }
}

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
    // Diagnostics must not interrupt product work.
  }
}

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
    // Diagnostics must not interrupt product work.
  }
}

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
    // Diagnostics must not interrupt product work.
  }
}

export type {
  ActionMilestones,
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
  startActionMilestones,
  startActionSpan,
  startCommandSpan,
  startRpcSpan,
  telemetryQueueLength,
  vueErrorHandler,
};
