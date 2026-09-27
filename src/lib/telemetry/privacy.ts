/** Reduces a source location to a basename and line/column, matching the
 * "sanitize stack traces to module or source basenames" redaction rule.
 * Directory components can carry a project path or a username, so they are
 * dropped rather than truncated. */
function sanitizeSourceLocation(
  file: string,
  line?: number,
  column?: number,
): string {
  const base = file.split(/[/\\]/).filter(Boolean).pop() ?? 'unknown';
  if (line === undefined) return base;
  return column === undefined ? `${base}:${line}` : `${base}:${line}:${column}`;
}

/** The hard per-attribute size ceiling every catalog entry in `attributes`
 * uses, so nothing accidentally inherits an unbounded length. */
const DEFAULT_MAX_ATTRIBUTE_LEN = 128;

const utf8Encoder = new TextEncoder();

function utf8Length(value: string): number {
  return utf8Encoder.encode(value).length;
}

/** Truncates `value` to at most `maxLen` UTF-8 bytes without splitting a
 * codepoint. This matches the native validator's length unit. */
function truncateToLimit(value: string, maxLen: number): string {
  if (utf8Length(value) <= maxLen) return value;

  let result = '';
  let length = 0;
  for (const codepoint of value) {
    const codepointLength = utf8Length(codepoint);
    if (length + codepointLength > maxLen) break;
    result += codepoint;
    length += codepointLength;
  }
  return result;
}

export {
  DEFAULT_MAX_ATTRIBUTE_LEN,
  sanitizeSourceLocation,
  truncateToLimit,
  utf8Length,
};
