/*
 * Forbidden-content canaries and redaction helpers shared by every stage's
 * privacy tests, so "does telemetry leak X" is answered against one list
 * instead of being redefined per test file. Mirrors
 * src-tauri/src/telemetry/privacy.rs.
 */

/** Distinct sentinel values later tests inject into the content categories
 * telemetry must never record, then scan persisted output for. Kept in one
 * place so a test that forgets a category shows up as a missing entry here,
 * not as a silently-absent assertion in a test body. */
const FORBIDDEN_CONTENT_CANARIES: ReadonlyMap<string, string> = new Map([
  ['prompt', 'tau-canary-prompt-3f1c9a'],
  ['assistantResponse', 'tau-canary-response-3f1c9a'],
  ['transcriptEntry', 'tau-canary-transcript-3f1c9a'],
  ['draft', 'tau-canary-draft-3f1c9a'],
  ['toolArguments', 'tau-canary-tool-args-3f1c9a'],
  ['toolResult', 'tau-canary-tool-result-3f1c9a'],
  ['extensionMessage', 'tau-canary-extension-3f1c9a'],
  ['clipboard', 'tau-canary-clipboard-3f1c9a'],
  ['fileContents', 'tau-canary-file-3f1c9a'],
  ['piRpcBody', 'tau-canary-rpc-body-3f1c9a'],
  ['sshCommand', 'tau-canary-ssh-3f1c9a'],
  ['credential', 'tau-canary-credential-3f1c9a'],
  ['projectPath', '/Users/tau-canary-3f1c9a/project'],
  ['connectionString', 'ssh://tau-canary-3f1c9a@build-box'],
]);

/** True if `haystack` contains any forbidden-content canary. Later stages'
 * privacy tests assert this is `false` for everything telemetry persists. */
function containsForbiddenContent(haystack: string): boolean {
  for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
    if (haystack.includes(canary)) return true;
  }
  return false;
}

/** Names of every canary found in `haystack`, so a failing test can report
 * which content categories leaked instead of just that something did. */
function matchedCanaries(haystack: string): string[] {
  return [...FORBIDDEN_CONTENT_CANARIES.entries()]
    .filter(([, canary]) => haystack.includes(canary))
    .map(([name]) => name);
}

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
  containsForbiddenContent,
  DEFAULT_MAX_ATTRIBUTE_LEN,
  FORBIDDEN_CONTENT_CANARIES,
  matchedCanaries,
  sanitizeSourceLocation,
  truncateToLimit,
  utf8Length,
};
