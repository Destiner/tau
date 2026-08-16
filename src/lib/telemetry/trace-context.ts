/*
 * The explicit trace-context argument Tauri commands accept once later
 * stages wire IPC propagation. Concurrent sessions and controllers pass this
 * value explicitly on each call; nothing here is stored as ambient global
 * state, so context cannot leak between them. Mirrors
 * src-tauri/src/telemetry/trace_context.rs.
 */

/** A decoded W3C `traceparent` (https://www.w3.org/TR/trace-context/).
 * `tracestate` is out of scope: Tau does not need vendor-specific state. */
interface TraceContext {
  /** 32 lowercase hex characters, not all zero. */
  traceId: string;
  /** 16 lowercase hex characters, not all zero. */
  spanId: string;
  sampled: boolean;
}

type TraceContextError = 'format' | 'version' | 'trace-id' | 'span-id';

type TraceContextResult =
  { ok: true; context: TraceContext } | { ok: false; error: TraceContextError };

const TRACE_ID_LENGTH = 32;
const SPAN_ID_LENGTH = 16;
const LOWER_HEX = /^[0-9a-f]+$/;

function isNonZeroLowerHex(value: string, expectedLength: number): boolean {
  return (
    value.length === expectedLength &&
    LOWER_HEX.test(value) &&
    !/^0+$/.test(value)
  );
}

/** Parses a `version-traceId-spanId-flags` traceparent header value. */
function parseTraceContext(traceparent: string): TraceContextResult {
  const parts = traceparent.split('-');
  if (parts.length !== 4) return { ok: false, error: 'format' };
  const [version, traceId, spanId, flags] = parts as [
    string,
    string,
    string,
    string,
  ];

  if (version !== '00') return { ok: false, error: 'version' };
  if (!isNonZeroLowerHex(traceId, TRACE_ID_LENGTH)) {
    return { ok: false, error: 'trace-id' };
  }
  if (!isNonZeroLowerHex(spanId, SPAN_ID_LENGTH)) {
    return { ok: false, error: 'span-id' };
  }
  if (flags.length !== 2 || !LOWER_HEX.test(flags)) {
    return { ok: false, error: 'format' };
  }

  const sampled = (Number.parseInt(flags, 16) & 0x01) !== 0;
  return { ok: true, context: { traceId, spanId, sampled } };
}

/** Formats a trace context back into a `traceparent` header value. */
function formatTraceContext(context: TraceContext): string {
  const flags = context.sampled ? '01' : '00';
  return `00-${context.traceId}-${context.spanId}-${flags}`;
}

export type { TraceContext, TraceContextError, TraceContextResult };

export { formatTraceContext, parseTraceContext };
