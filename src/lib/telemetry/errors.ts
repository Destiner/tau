/*
 * Pure classification helpers for Stage 4's frontend error capture. No
 * queue/state access here: `index.ts` owns recording spans/logs, this
 * module only turns an unknown thrown or rejected value into a bounded,
 * content-free category and a sanitized source location. Every function
 * here is a pure function of its argument, so it is testable without a
 * DOM (no `window`/`ErrorEvent` needed).
 */
import { FRONTEND_ERROR_KINDS, type FrontendErrorKind } from './attributes';
import { sanitizeSourceLocation } from './privacy';

const NAMED_ERROR_KINDS: readonly string[] = FRONTEND_ERROR_KINDS.filter(
  (kind) => kind !== 'other' && kind !== 'none',
);

/** Categorizes an unknown thrown/rejected value by its constructor name
 * against the reviewed, bounded allowlist. Never reads `.message` or
 * serializes the value itself — only `instanceof Error` and `.name`, both
 * of which reveal nothing about the value's content. */
function classifyErrorKind(value: unknown): FrontendErrorKind {
  if (value === undefined) return 'none';
  if (value instanceof Error && NAMED_ERROR_KINDS.includes(value.name)) {
    return value.name as FrontendErrorKind;
  }
  return 'other';
}

/** Extracts the first stack frame's `file:line:column` from an Error's
 * `.stack` (the line after the leading "Error: message" line in V8's
 * format), then sanitizes it to a basename. Never reads the stack's first
 * line, which repeats the error's own message. Best-effort: an unfamiliar
 * stack format yields an empty string rather than a wrong guess. */
function locationFromStack(stack: string): string {
  const frame = stack.split('\n')[1] ?? '';
  const match = /([^\s():]+):(\d+):(\d+)/.exec(frame);
  if (!match) return '';
  const [, file, line, column] = match;
  return sanitizeSourceLocation(file ?? '', Number(line), Number(column));
}

/** Sanitized source location for an unknown thrown/rejected value, or an
 * empty string when none is available. Only ever reads `.stack`. */
function locationFromValue(value: unknown): string {
  if (value instanceof Error && typeof value.stack === 'string') {
    return locationFromStack(value.stack);
  }
  return '';
}

/** Sanitized source location for a `window.onerror`/`ErrorEvent`-shaped
 * object, preferring the event's own structured `filename`/`lineno`/
 * `colno` (always populated by the browser, even for a thrown non-Error
 * value) over parsing a stack string. Takes a structural type rather than
 * the real `ErrorEvent` so this stays testable without a DOM. */
function locationFromErrorEvent(event: {
  filename?: string;
  lineno?: number;
  colno?: number;
  error?: unknown;
}): string {
  if (event.filename) {
    return sanitizeSourceLocation(
      event.filename,
      event.lineno || undefined,
      event.colno || undefined,
    );
  }
  return locationFromValue(event.error);
}

export {
  classifyErrorKind,
  locationFromErrorEvent,
  locationFromStack,
  locationFromValue,
};
