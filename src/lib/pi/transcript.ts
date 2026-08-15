interface TranscriptEntry {
  id: string;
  /** An `error` entry holds Pi's own error string in `text`, unparsed. */
  kind: 'user' | 'assistant' | 'thinking' | 'tool' | 'error';
  text: string;
  toolCallId?: string;
  toolName?: string;
  toolRunning?: boolean;
  toolErrored?: boolean;
  toolArguments?: string;
  toolResult?: string;
}

type JsonRecord = Record<string, unknown>;

/**
 * How much of a call's arguments and result a row carries. The expansion is a
 * look at what the tool was asked and what came back, not a file viewer, and
 * every tool row in a long transcript holds its own copy.
 */
const detailLimit = 4_000;

/**
 * Rebuilds the transcript from Pi's message list. `previous` holds the entries
 * being replaced so their ids can carry over: a settled turn rewrites rows the
 * reader may be looking at, and a fresh id costs the virtualizer the height it
 * measured for that row, which collapses the transcript under a reader who has
 * scrolled up and drags them to the bottom.
 */
function hydrateTranscript(
  messages: unknown[],
  previous: TranscriptEntry[] = [],
): TranscriptEntry[] {
  const entries: TranscriptEntry[] = [];

  for (const value of messages) {
    const message = asRecord(value);
    if (!message) continue;
    const role = stringValue(message.role);

    if (role === 'user') {
      const text = contentText(message.content);
      if (text) entries.push({ id: '', kind: 'user', text });
      continue;
    }

    if (role === 'assistant') {
      const content = Array.isArray(message.content) ? message.content : [];
      const failure = messageFailure(message);
      for (const partValue of content) {
        const part = asRecord(partValue);
        if (!part) continue;
        const type = stringValue(part.type);
        if (type === 'text') {
          const text = stringValue(part.text);
          if (text) entries.push({ id: '', kind: 'assistant', text });
        } else if (type === 'thinking') {
          const text = stringValue(part.thinking);
          if (text) entries.push({ id: '', kind: 'thinking', text });
        } else if (type === 'toolCall') {
          const toolCallId = stringValue(part.id);
          const toolName = stringValue(part.name) || 'tool';
          entries.push({
            id: '',
            kind: 'tool',
            text: toolSummary(part.arguments),
            toolCallId,
            toolName,
            toolRunning: true,
            toolErrored: false,
            toolArguments: toolArgumentsText(part.arguments),
          });
        }
      }
      // A turn that failed carries its reason beside content that is usually
      // empty, so the row stands for the reply the reader never got.
      if (failure) entries.push({ id: '', kind: 'error', text: failure });
      continue;
    }

    if (role === 'toolResult') {
      const toolCallId = stringValue(message.toolCallId);
      const existing = [...entries]
        .reverse()
        .find(
          (entry) => entry.kind === 'tool' && entry.toolCallId === toolCallId,
        );
      if (existing) {
        existing.toolRunning = false;
        existing.toolErrored = message.isError === true;
        existing.toolResult = toolResultText(message.content);
      } else {
        const toolName = stringValue(message.toolName) || 'tool';
        entries.push({
          id: '',
          kind: 'tool',
          text: '',
          toolCallId,
          toolName,
          toolRunning: false,
          toolErrored: message.isError === true,
          toolResult: toolResultText(message.content),
        });
      }
      continue;
    }

    if (role === 'bashExecution') {
      const command = stringValue(message.command);
      entries.push({
        id: '',
        kind: 'tool',
        text: command,
        toolName: 'bash',
        toolRunning: false,
        toolErrored: numberValue(message.exitCode) !== 0,
        toolResult: toolResultText(message.output),
      });
    }
  }
  return assignIds(entries, previous);
}

function assignIds(
  entries: TranscriptEntry[],
  previous: TranscriptEntry[],
): TranscriptEntry[] {
  const adopted = new Map<number, string>();
  let cursor = 0;
  entries.forEach((entry, index) => {
    const candidate = previous[cursor];
    if (!candidate || !holdsSameRow(entry, candidate)) return;
    adopted.set(index, candidate.id);
    cursor += 1;
  });

  const taken = new Set(adopted.values());
  let sequence = 0;
  entries.forEach((entry, index) => {
    const carried = adopted.get(index);
    if (carried) {
      entry.id = carried;
      return;
    }
    let id = `${entry.kind}-${sequence++}`;
    while (taken.has(id)) id = `${entry.kind}-${sequence++}`;
    entry.id = id;
  });
  return entries;
}

/**
 * Entries hold the same row when they occupy the same slot in the same order.
 * Content may have been refined since it streamed — a partial reply completed,
 * a tool call resolved — but the reader is looking at the row either way.
 */
function holdsSameRow(
  entry: TranscriptEntry,
  candidate: TranscriptEntry,
): boolean {
  if (entry.kind !== candidate.kind) return false;
  if (!entry.toolCallId || !candidate.toolCallId) return true;
  return entry.toolCallId === candidate.toolCallId;
}

/**
 * The reason a turn failed, or nothing. An aborted turn also carries a reason,
 * and it is the reader's own stop rather than a failure to report.
 */
function messageFailure(message: unknown): string {
  const record = asRecord(message);
  if (!record || stringValue(record.role) !== 'assistant') return '';
  if (stringValue(record.stopReason) !== 'error') return '';
  return stringValue(record.errorMessage);
}

/** Appends the failures Pi reports as events but never keeps in its messages. */
function appendLocalErrors(
  entries: TranscriptEntry[],
  errors: string[],
): TranscriptEntry[] {
  errors.forEach((text, index) => {
    entries.push({ id: `local-error-${index}`, kind: 'error', text });
  });
  return entries;
}

/** The single line a collapsed tool row shows: whichever argument names the call. */
function toolSummary(args: unknown): string {
  const record = asRecord(args);
  return (
    stringValue(record?.command) ||
    stringValue(record?.path) ||
    stringValue(record?.file_path) ||
    stringValue(record?.query) ||
    compactJson(args)
  );
}

function toolArgumentsText(args: unknown): string {
  const record = asRecord(args);
  if (!record || Object.keys(record).length === 0) return '';
  try {
    return clampDetail(JSON.stringify(record, null, 2) ?? '');
  } catch {
    return '';
  }
}

function toolResultText(content: unknown): string {
  return clampDetail(contentText(content));
}

function clampDetail(text: string): string {
  return text.length > detailLimit ? `${text.slice(0, detailLimit)}\n…` : text;
}

function contentText(value: unknown): string {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return '';
  return value
    .map((part) => {
      const record = asRecord(part);
      return record && record.type === 'text' ? stringValue(record.text) : '';
    })
    .filter(Boolean)
    .join('\n');
}

function asRecord(value: unknown): JsonRecord | undefined {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as JsonRecord)
    : undefined;
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined;
}

function compactJson(value: unknown): string {
  if (value === undefined) return '';
  try {
    const text = JSON.stringify(value);
    return text.length > 220 ? `${text.slice(0, 217)}…` : text;
  } catch {
    return '';
  }
}

export type { TranscriptEntry };

export {
  hydrateTranscript,
  messageFailure,
  appendLocalErrors,
  toolSummary,
  toolArgumentsText,
  toolResultText,
  contentText,
  asRecord,
  stringValue,
};
