import { describePiError } from './error';
import type { PiErrorDescription } from './error';

type TranscriptNoticeType = 'info' | 'warning' | 'error';

interface TranscriptEntry {
  id: string;
  kind:
    | 'user'
    | 'assistant'
    | 'thinking'
    | 'tool'
    | 'skill'
    | 'error'
    | 'notice'
    | 'compaction';
  text: string;
  /** Tau rendered this row before Pi confirmed it as session content. */
  pending?: boolean;
  /** Whether activating this compaction boundary can reveal an older layer. */
  historyAvailable?: boolean;
  /** The older layer behind this boundary is currently being requested. */
  historyLoading?: boolean;
  /** Reviewed label for a plain-language error entry. */
  errorLabel?: string;
  /**
   * Set on the entries Tau owns rather than Pi: how many Pi-owned rows preceded
   * the entry when it arrived, so a rebuilt transcript can restore it there.
   */
  anchor?: number;
  noticeType?: TranscriptNoticeType;
  /** Base for file paths in extension notices; absent for remote projects. */
  basePath?: string;
  toolCallId?: string;
  toolName?: string;
  toolRunning?: boolean;
  toolErrored?: boolean;
  toolArguments?: string;
  toolResult?: string;
  skillName?: string;
  skillPrompt?: string;
}

/** A failure Pi reported as an event, held with the spot it belongs in. */
interface LocalError {
  /** Names the row across rebuilds, and stays unique as runs come and go. */
  key: number;
  label: string;
  text: string;
  anchor?: number;
}

/** The id a failure keeps whether it is streamed in or merged back. */
function localErrorId(key: number): string {
  return `local-error-${key}`;
}

interface ParsedSkillBlock {
  name: string;
  location: string;
  content: string;
  userMessage?: string;
}

interface HistoryLayer {
  /** Stable id for the compaction boundary immediately before this layer. */
  markerId: string;
  rows: TranscriptEntry[];
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
      const skill = parseSkillBlock(text);
      if (skill) {
        entries.push({
          id: '',
          kind: 'skill',
          text: skill.content,
          skillName: skill.name,
          ...(skill.userMessage ? { skillPrompt: skill.userMessage } : {}),
        });
      } else if (text) {
        entries.push({ id: '', kind: 'user', text });
      }
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
      if (failure) {
        entries.push({
          id: '',
          kind: 'error',
          text: failure.message,
          errorLabel: failure.label,
        });
      }
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
      continue;
    }

    if (role === 'compactionSummary') {
      entries.push({
        id: '',
        kind: 'compaction',
        text: '',
        historyAvailable: true,
        historyLoading: false,
      });
    }
  }
  return assignIds(entries, previous);
}

/**
 * Splits the raw history hidden by the latest compaction into progressively
 * older layers. Pi owns the append-only tree; Tau only projects the active
 * branch and leaves the current compacted tail to `get_messages`.
 */
function historyLayersFromEntries(
  values: unknown[],
  leafId: string,
): HistoryLayer[] {
  const entries = values
    .map(asRecord)
    .filter((entry): entry is JsonRecord => Boolean(entry));
  const byId = new Map(
    entries
      .map((entry) => [stringValue(entry.id), entry] as const)
      .filter(([id]) => Boolean(id)),
  );
  const path: JsonRecord[] = [];
  const seen = new Set<string>();
  let cursor = leafId;
  while (cursor && !seen.has(cursor)) {
    seen.add(cursor);
    const entry = byId.get(cursor);
    if (!entry) return [];
    path.push(entry);
    cursor = stringValue(entry.parentId);
  }
  path.reverse();

  const indexById = new Map(
    path.map((entry, index) => [stringValue(entry.id), index]),
  );
  const compactions = path
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => entry.type === 'compaction');
  const latest = compactions[compactions.length - 1];
  const latestStart = latest
    ? indexById.get(stringValue(latest.entry.firstKeptEntryId))
    : undefined;
  if (latestStart === undefined || latestStart <= 0) return [];

  const cutoffs = new Set<number>([0, latestStart]);
  for (const { entry, index } of compactions) {
    if (index >= (latest?.index ?? 0)) continue;
    const cutoff = indexById.get(stringValue(entry.firstKeptEntryId));
    if (cutoff !== undefined && cutoff > 0 && cutoff < latestStart) {
      cutoffs.add(cutoff);
    }
  }
  const ordered = [...cutoffs].sort((left, right) => left - right);
  const layers: HistoryLayer[] = [];
  for (let index = 0; index < ordered.length - 1; index += 1) {
    const start = ordered[index] ?? 0;
    const end = ordered[index + 1] ?? latestStart;
    const messages = path
      .slice(start, end)
      .filter((entry) => entry.type === 'message')
      .map((entry) => entry.message);
    const rowPrefix = stringValue(path[start]?.id) || `root-${index}`;
    const rows = hydrateTranscript(messages).map((row) => ({
      ...row,
      id: `history-${rowPrefix}-${row.id}`,
      historyAvailable: false,
      historyLoading: false,
    }));
    if (rows.length === 0) continue;
    layers.push({
      markerId: `history-compaction-${rowPrefix}`,
      rows,
    });
  }
  return layers;
}

/** The loaded prefix for one reveal depth, including every boundary it spans. */
function historyPrefix(
  layers: HistoryLayer[],
  firstVisibleLayer: number,
): TranscriptEntry[] {
  const prefix: TranscriptEntry[] = [];
  if (firstVisibleLayer > 0) {
    prefix.push({
      id: layers[firstVisibleLayer]?.markerId ?? 'history-compaction',
      kind: 'compaction',
      text: '',
      historyAvailable: true,
      historyLoading: false,
    });
  }
  for (let index = firstVisibleLayer; index < layers.length; index += 1) {
    if (index > firstVisibleLayer) {
      prefix.push({
        id: layers[index]?.markerId ?? `history-compaction-${index}`,
        kind: 'compaction',
        text: '',
        historyAvailable: false,
        historyLoading: false,
      });
    }
    prefix.push(...(layers[index]?.rows ?? []));
  }
  return prefix;
}

function assignIds(
  entries: TranscriptEntry[],
  previous: TranscriptEntry[],
): TranscriptEntry[] {
  // Only Pi-owned rows take part: the local entries sitting between them are
  // merged back afterwards, and counting them here would stall the walk at the
  // first one and cost every row below it the height the virtualizer measured.
  const candidates = previous.filter((entry) => !isLocalEntry(entry));
  const adopted = new Map<number, string>();
  let cursor = 0;
  entries.forEach((entry, index) => {
    const candidate = candidates[cursor];
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
  if (entry.kind === 'skill') return entry.skillName === candidate.skillName;
  if (!entry.toolCallId || !candidate.toolCallId) return true;
  return entry.toolCallId === candidate.toolCallId;
}

/**
 * The reason a turn failed, or nothing. An aborted turn also carries a reason,
 * and it is the reader's own stop rather than a failure to report.
 */
function messageFailure(message: unknown): PiErrorDescription | undefined {
  const record = asRecord(message);
  if (!record || stringValue(record.role) !== 'assistant') return undefined;
  if (stringValue(record.stopReason) !== 'error') return undefined;
  const raw = stringValue(record.errorMessage);
  return raw ? describePiError(raw) : undefined;
}

/** Entries Tau carries itself, because Pi's message list cannot hold them. */
function isLocalEntry(entry: TranscriptEntry): boolean {
  return entry.kind === 'notice' || entry.anchor !== undefined;
}

/**
 * Restores the entries Tau owns into a freshly hydrated transcript: the
 * failures Pi reports as events but never keeps in its messages, and the
 * fire-and-forget notices its extensions raise. Each one holds the number of
 * Pi-owned rows that preceded it and goes back at that spot, because appending
 * them instead walks them to the bottom of the transcript on every rebuild.
 *
 * An entry with no spot to claim is given the one it lands in, on the entry
 * itself and on the failure it was built from, so a row that settled at the
 * bottom of a transcript it preceded stays there instead of following the tail.
 */
function mergeLocalEntries(
  entries: TranscriptEntry[],
  errors: LocalError[],
  previous: TranscriptEntry[],
): TranscriptEntry[] {
  const held = new Map(errors.map((error) => [localErrorId(error.key), error]));
  const locals: TranscriptEntry[] = errors.map((error) => ({
    id: localErrorId(error.key),
    kind: 'error',
    text: error.text,
    errorLabel: error.label,
    ...(error.anchor === undefined ? {} : { anchor: error.anchor }),
  }));
  const ids = new Set([...entries, ...locals].map((entry) => entry.id));
  for (const entry of previous) {
    if (entry.kind !== 'notice' || ids.has(entry.id)) continue;
    locals.push(entry);
    ids.add(entry.id);
  }
  if (locals.length === 0) return entries;

  const ordered = locals
    .map((entry, index) => ({ entry, index, anchor: anchorOf(entry, entries) }))
    // Entries that share a spot keep the order they arrived in.
    .sort(
      (left, right) => left.anchor - right.anchor || left.index - right.index,
    );

  const merged: TranscriptEntry[] = [];
  let cursor = 0;
  for (const { entry, anchor } of ordered) {
    // Resolving the spot here is what keeps an entry that arrived before the
    // transcript had loaded from drifting again on the rebuild after this one.
    const error = held.get(entry.id);
    if (error) error.anchor = anchor;
    merged.push(...entries.slice(cursor, anchor), { ...entry, anchor });
    cursor = anchor;
  }
  merged.push(...entries.slice(cursor));
  return merged;
}

/**
 * Where a local entry belongs. An entry that arrived before any Pi-owned row
 * was known has no spot to hold, and neither has one whose spot a compaction
 * has since dropped; both settle at the bottom of the transcript as it stands.
 */
function anchorOf(entry: TranscriptEntry, entries: TranscriptEntry[]): number {
  return Math.min(entry.anchor ?? entries.length, entries.length);
}

/** Parses the exact user-message envelope Pi records for `/skill:name`. */
function parseSkillBlock(text: string): ParsedSkillBlock | undefined {
  const match =
    /^<skill name="([^"]+)" location="([^"]+)">\n([\s\S]*?)\n<\/skill>(?:\n\n([\s\S]+))?$/.exec(
      text,
    );
  if (!match) return undefined;
  return {
    name: match[1] ?? '',
    location: match[2] ?? '',
    content: match[3] ?? '',
    ...(match[4]?.trim() ? { userMessage: match[4].trim() } : {}),
  };
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

export type {
  HistoryLayer,
  LocalError,
  ParsedSkillBlock,
  TranscriptEntry,
  TranscriptNoticeType,
};

export {
  hydrateTranscript,
  historyLayersFromEntries,
  historyPrefix,
  localErrorId,
  parseSkillBlock,
  messageFailure,
  mergeLocalEntries,
  toolSummary,
  toolArgumentsText,
  toolResultText,
  contentText,
  asRecord,
  stringValue,
};
