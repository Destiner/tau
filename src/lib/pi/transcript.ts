import { describePiError } from './error';
import type { PiErrorDescription } from './error';

type TranscriptNoticeType = 'info' | 'warning' | 'error';

/** Ephemeral projection identity, never a Pi session-file identifier. */
interface TranscriptSource {
  message: string;
  block: number;
  offset: number;
}

let projectionSequence = 0;

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

  pending?: boolean;

  pendingUserEvent?: 'optimistic' | 'hydration';

  historyAvailable?: boolean;

  historyLoading?: boolean;

  errorLabel?: string;

  anchor?: number;
  boundary?: TranscriptSource;
  afterToolCallId?: string;
  source?: TranscriptSource;
  noticeType?: TranscriptNoticeType;

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

interface LocalError {
  key: number;
  label: string;
  text: string;
  anchor?: number;
  boundary?: TranscriptSource;
  afterToolCallId?: string;
}

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
 * Rebuilds the transcript from Pi's message list. `previous` holds the entries
 * being replaced so their ids can carry over: a settled turn rewrites rows the
 * reader may be looking at, and a fresh id costs the virtualizer the height it
 * measured for that row, which collapses the transcript under a reader who has
 * scrolled up and drags them to the bottom.
 */
function hydrateTranscript(
  messages: unknown[],
  previous: TranscriptEntry[] = [],
  pendingLiveUserEvents = false,
): TranscriptEntry[] {
  const entries: TranscriptEntry[] = [];

  for (const value of messages) {
    const message = asRecord(value);
    if (!message) continue;
    const role = stringValue(message.role);
    const sourceMessage = `projection-${projectionSequence++}`;
    const add = (entry: TranscriptEntry, block = 0): void => {
      entries.push({
        ...entry,
        source: { message: sourceMessage, block, offset: 0 },
      });
    };

    if (role === 'user') {
      const text = contentText(message.content);
      const skill = parseSkillBlock(text);
      if (skill) {
        add({
          id: '',
          kind: 'skill',
          text: skill.content,
          skillName: skill.name,
          ...(skill.userMessage ? { skillPrompt: skill.userMessage } : {}),
        });
      } else if (text) {
        add({ id: '', kind: 'user', text });
      }
      continue;
    }

    if (role === 'assistant') {
      const content = Array.isArray(message.content) ? message.content : [];
      const stopReason = stringValue(message.stopReason);
      const terminalToolCall =
        stopReason === 'aborted' || stopReason === 'error';
      const failure = messageFailure(message);
      for (const [block, partValue] of content.entries()) {
        const part = asRecord(partValue);
        if (!part) continue;
        const type = stringValue(part.type);
        if (type === 'text') {
          const text = stringValue(part.text);
          if (text) add({ id: '', kind: 'assistant', text }, block);
        } else if (type === 'thinking') {
          const text = stringValue(part.thinking);
          if (text) add({ id: '', kind: 'thinking', text }, block);
        } else if (type === 'toolCall') {
          const toolCallId = stringValue(part.id);
          const toolName = stringValue(part.name) || 'tool';
          add(
            {
              id: '',
              kind: 'tool',
              text: toolSummary(part.arguments),
              toolCallId,
              toolName,
              // A terminal turn cannot still be running without a recorded result.
              toolRunning: !terminalToolCall,
              toolErrored: terminalToolCall,
              toolArguments: toolArgumentsText(part.arguments),
            },
            block,
          );
        }
      }
      // A turn that failed carries its reason beside content that is usually
      // empty, so the row stands for the reply the reader never got.
      if (failure) {
        add(
          {
            id: '',
            kind: 'error',
            text: failure.message,
            errorLabel: failure.label,
          },
          content.length,
        );
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
        add({
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
      add({
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
      add({
        id: '',
        kind: 'compaction',
        text: '',
        historyAvailable: true,
        historyLoading: false,
      });
    }
  }
  return restoreSourceSegments(
    assignIds(entries, previous, pendingLiveUserEvents),
    previous,
  );
}

/**
 * Projects one ordinary user start event. Only an explicitly pending composer
 * row or a hydration row waiting for this same live event may be reconciled;
 * equal text on an already completed turn is not provenance for deduplication.
 */
function projectOrdinaryUserMessage(
  entries: TranscriptEntry[],
  text: string,
  id: string,
): void {
  if (!text) return;

  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const candidate = entries[index];
    if (!candidate) continue;
    if (isLocalEntry(candidate)) continue;
    if (
      candidate.kind === 'user' &&
      (candidate.pending === true ||
        candidate.pendingUserEvent === 'optimistic')
    ) {
      candidate.text = text;
      delete candidate.pending;
      delete candidate.pendingUserEvent;
      return;
    }
    break;
  }

  const hydrated = entries.find(
    (entry) =>
      entry.kind === 'user' &&
      entry.pendingUserEvent === 'hydration' &&
      entry.text === text,
  );
  if (hydrated) {
    delete hydrated.pendingUserEvent;
    return;
  }

  entries.push({ id, kind: 'user', text });
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

function sameSource(
  left?: TranscriptSource,
  right?: TranscriptSource,
): boolean {
  return Boolean(
    left &&
    right &&
    left.message === right.message &&
    left.block === right.block,
  );
}

/** Restore only proven offsets; refinements fall back inside the owning block. */
function restoreSourceSegments(
  entries: TranscriptEntry[],
  previous: TranscriptEntry[],
): TranscriptEntry[] {
  const segments = new Map<string, TranscriptEntry[]>();
  const boundaries = new Map<string, number[]>();
  for (const entry of previous) {
    if (entry.boundary) {
      const key = `${entry.boundary.message}:${entry.boundary.block}`;
      const offsets = boundaries.get(key) ?? [];
      offsets.push(entry.boundary.offset);
      boundaries.set(key, offsets);
    }
  }
  for (const entry of previous) {
    if (!entry.source || isLocalEntry(entry)) continue;
    const key = `${entry.source.message}:${entry.source.block}`;
    const group = segments.get(key) ?? [];
    group.push(entry);
    segments.set(key, group);
  }
  return entries.flatMap((entry) => {
    if (
      !entry.source ||
      (entry.kind !== 'assistant' && entry.kind !== 'thinking')
    )
      return [entry];
    const group =
      segments.get(`${entry.source.message}:${entry.source.block}`) ?? [];
    const oldText = group.map((segment) => segment.text).join('');
    const result: TranscriptEntry[] = [];
    let offset = 0;
    let id = entry.id;
    const cuts = new Set([
      ...group.slice(1).map((segment) => segment.source!.offset),
      ...(boundaries.get(`${entry.source.message}:${entry.source.block}`) ??
        []),
    ]);
    for (const cut of [...cuts].sort((a, b) => a - b)) {
      if (
        cut <= offset ||
        cut >= entry.text.length ||
        cut > oldText.length ||
        entry.text.slice(0, cut) !== oldText.slice(0, cut)
      )
        continue;
      result.push({
        ...entry,
        id,
        text: entry.text.slice(offset, cut),
        source: { ...entry.source, offset },
      });
      offset = cut;
      id =
        group.find((segment) => segment.source!.offset === cut)?.id ??
        `${entry.id}-segment-${cut}`;
    }
    result.push({
      ...entry,
      id,
      text: entry.text.slice(offset),
      source: { ...entry.source, offset },
    });
    return result;
  });
}

function assignIds(
  entries: TranscriptEntry[],
  previous: TranscriptEntry[],
  pendingLiveUserEvents: boolean,
): TranscriptEntry[] {
  // Only Pi-owned rows take part: the local entries sitting between them are
  // merged back afterwards, and counting them here would stall the walk at the
  // first one and cost every row below it the height the virtualizer measured.
  const candidates: TranscriptEntry[] = [];
  for (const entry of previous.filter((entry) => !isLocalEntry(entry))) {
    const last = candidates[candidates.length - 1];
    if (last && sameSource(last.source, entry.source)) {
      last.text += entry.text;
    } else {
      candidates.push({ ...entry });
    }
  }
  const adopted = new Map<number, string>();
  let cursor = 0;
  let projectedMessage: string | undefined;
  let previousMessage: string | undefined;
  entries.forEach((entry, index) => {
    if (entry.source?.message !== projectedMessage) {
      // A refinement may remove blocks, but cannot donate them to the next message.
      while (
        previousMessage &&
        candidates[cursor]?.source?.message === previousMessage
      )
        cursor++;
      projectedMessage = entry.source?.message;
      const candidate = candidates[cursor];
      const messageKind = (row: TranscriptEntry): string =>
        row.kind === 'user' || row.kind === 'skill'
          ? 'user'
          : row.kind === 'compaction'
            ? 'compaction'
            : 'assistant';
      previousMessage =
        candidate && messageKind(candidate) === messageKind(entry)
          ? candidate.source?.message
          : undefined;
    }
    if (entry.source && previousMessage) {
      while (
        candidates[cursor]?.source?.message === previousMessage &&
        candidates[cursor]!.source!.block < entry.source.block
      )
        cursor++;
      entry.source = { ...entry.source, message: previousMessage };
    }
    const candidate = candidates[cursor];
    if (!candidate || !holdsSameRow(entry, candidate)) return;
    if (
      entry.source &&
      candidate.source &&
      (candidate.source.message !== previousMessage ||
        candidate.source.block !== entry.source.block)
    )
      return;
    adopted.set(index, candidate.id);
    cursor += 1;
  });

  const taken = new Set([
    ...adopted.values(),
    ...previous
      .filter((entry) => (entry.source?.offset ?? 0) > 0)
      .map((entry) => entry.id),
  ]);
  const candidatesById = new Map(
    candidates.map((candidate) => [candidate.id, candidate]),
  );
  let liveUserSuffixStart = entries.length;
  if (pendingLiveUserEvents) {
    while (
      liveUserSuffixStart > 0 &&
      entries[liveUserSuffixStart - 1]?.kind === 'user'
    ) {
      liveUserSuffixStart -= 1;
    }
  }
  let sequence = 0;
  entries.forEach((entry, index) => {
    const carried = adopted.get(index);
    if (carried) {
      entry.id = carried;
      const candidate = candidatesById.get(carried);
      if (candidate?.source) entry.source = { ...candidate.source, offset: 0 };
      if (pendingLiveUserEvents && candidate?.pendingUserEvent) {
        entry.pendingUserEvent = candidate.pendingUserEvent;
      }
      return;
    }
    let id = `${entry.kind}-${sequence++}`;
    while (taken.has(id)) id = `${entry.kind}-${sequence++}`;
    entry.id = id;
    if (entry.kind === 'user' && index >= liveUserSuffixStart) {
      entry.pendingUserEvent = 'hydration';
    }
  });

  // Pi can answer a mid-start hydration before it has recorded the user event
  // Tau already projected. Keep that row until a later settled hydration is
  // authoritative, rather than blinking it out and recreating it from the event.
  if (pendingLiveUserEvents) {
    const presentIds = new Set(entries.map((entry) => entry.id));
    for (const candidate of candidates) {
      if (
        candidate.kind === 'user' &&
        (candidate.pending || candidate.pendingUserEvent) &&
        !presentIds.has(candidate.id)
      ) {
        entries.push({ ...candidate });
      }
    }
  }
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
 * fire-and-forget notices its extensions raise. Source boundaries survive row
 * coalescing; numeric tail coordinates cover feedback without known provenance.
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
  const previousIds = new Set(previous.map((entry) => entry.id));
  const localErrorEntry = (error: LocalError): TranscriptEntry => ({
    id: localErrorId(error.key),
    kind: 'error',
    text: error.text,
    errorLabel: error.label,
    ...(error.anchor === undefined ? {} : { anchor: error.anchor }),
    ...(error.boundary ? { boundary: error.boundary } : {}),
    ...(error.afterToolCallId
      ? { afterToolCallId: error.afterToolCallId }
      : {}),
  });
  const locals: TranscriptEntry[] = errors
    .filter((error) => !previousIds.has(localErrorId(error.key)))
    .map(localErrorEntry);
  const ids = new Set([...entries, ...locals].map((entry) => entry.id));
  for (const entry of previous) {
    if (ids.has(entry.id)) continue;
    const error = held.get(entry.id);
    if (entry.kind !== 'notice' && !error) continue;
    locals.push(error ? localErrorEntry(error) : entry);
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
    const predecessor = entries[anchor - 1];
    const boundary = predecessor?.source
      ? {
          ...predecessor.source,
          offset: predecessor.source.offset + predecessor.text.length,
        }
      : entry.boundary;
    if (error) {
      error.anchor = anchor;
      error.boundary = boundary;
    }
    merged.push(...entries.slice(cursor, anchor), {
      ...entry,
      anchor,
      ...(boundary ? { boundary } : {}),
    });
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
  if (entry.afterToolCallId) {
    const tool = entries.findIndex(
      (candidate) =>
        candidate.kind === 'tool' &&
        candidate.toolCallId === entry.afterToolCallId,
    );
    if (tool >= 0) return tool + 1;
  }
  if (entry.boundary) {
    let last = -1;
    for (let index = 0; index < entries.length; index++) {
      const candidate = entries[index]!;
      if (!sameSource(candidate.source, entry.boundary)) continue;
      last = index;
      if (
        candidate.source!.offset + candidate.text.length >=
        entry.boundary.offset
      )
        return index + 1;
    }
    if (last >= 0) return last + 1;
    // A removed/refined block must not push feedback into a later message.
    for (let index = entries.length - 1; index >= 0; index--) {
      if (entries[index]?.source?.message === entry.boundary.message)
        return index + 1;
    }
  }
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
    return JSON.stringify(record, null, 2) ?? '';
  } catch {
    return '';
  }
}

function toolResultText(content: unknown): string {
  return contentText(content);
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
  TranscriptSource,
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
  projectOrdinaryUserMessage,
  messageFailure,
  mergeLocalEntries,
  toolSummary,
  toolArgumentsText,
  toolResultText,
  contentText,
  asRecord,
  stringValue,
};
