/**
 * A failed turn arrives from Pi as one string: a short prefix naming the
 * provider and status, then the provider's own JSON payload verbatim. The
 * sentence a reader needs is inside that payload, so the string is reduced to
 * the two parts worth showing rather than printed whole.
 */
interface PiErrorDescription {
  /** The provider and status, e.g. `OpenAI API error (401)`. */
  label: string;
  /** The sentence to read, taken from the payload when it carries one. */
  message: string;
}

/** How much of a message a row shows before the rest is left to the session file. */
const messageLimit = 400;

function describePiError(raw: string): PiErrorDescription {
  const text = raw.trim();
  if (!text) return { label: 'Error', message: 'Pi reported an error.' };

  const payload = parseTrailingJson(text);
  const message = payload ? payloadMessage(payload.value) : '';
  if (!message) return { label: 'Error', message: clamp(firstLine(text)) };

  return {
    label: labelFrom(payload?.prefix ?? '') || 'Error',
    message: clamp(message),
  };
}

/**
 * The payload runs to the end of the string, so the first brace opens it. A
 * provider that wraps its JSON in more prose parses as nothing and falls back
 * to the raw string, which the row shows as it stands.
 */
function parseTrailingJson(
  text: string,
): { prefix: string; value: unknown } | undefined {
  const start = text.indexOf('{');
  if (start < 0) return undefined;
  try {
    return {
      prefix: text.slice(0, start),
      value: JSON.parse(text.slice(start)),
    };
  } catch {
    return undefined;
  }
}

function payloadMessage(value: unknown): string {
  const record = asRecord(value);
  if (!record) return '';
  if (typeof record.message === 'string') return record.message.trim();
  const nested = record.error;
  if (typeof nested === 'string') return nested.trim();
  const nestedRecord = asRecord(nested);
  if (nestedRecord && typeof nestedRecord.message === 'string') {
    return nestedRecord.message.trim();
  }
  return '';
}

/**
 * Trims the separator the prefix ends on, and the word every row already says.
 * A prefix that is only a status code is named as one, since a number alone in
 * the label position reads as a count of something.
 */
function labelFrom(prefix: string): string {
  const label = prefix
    .trim()
    .replace(/[:\-–—]+$/, '')
    .replace(/^error:?\s*/i, '')
    .trim();
  return /^\d{3}$/.test(label) ? `HTTP ${label}` : label;
}

function firstLine(text: string): string {
  const [line] = text.split('\n');
  return (line ?? text).trim();
}

function clamp(text: string): string {
  return text.length > messageLimit
    ? `${text.slice(0, messageLimit).trimEnd()}…`
    : text;
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

export type { PiErrorDescription };

export { describePiError };
