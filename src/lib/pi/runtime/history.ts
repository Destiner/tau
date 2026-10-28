import {
  isControllerSelected,
  type SessionController,
} from '../../../composables/state';
import { describePiError, retryPiErrorMessage } from '../error';
import {
  hydrateTranscript,
  historyPrefix,
  localErrorId,
  stringValue,
  type LocalError,
  type TranscriptEntry,
  type TranscriptSource,
} from '../transcript';

function resetHistory(controller: SessionController): void {
  for (const entry of controller.messages) {
    if (entry.kind !== 'compaction') continue;
    entry.historyAvailable = false;
    entry.historyLoading = false;
  }
  controller.historyLayers = [];
  controller.firstVisibleHistoryLayer = 0;
  controller.historyPrefixLength = 0;
  controller.historyRequestId = '';
}

function setHistoryLoading(
  controller: SessionController,
  loading: boolean,
): void {
  const marker = controller.messages.find(
    (entry) => entry.kind === 'compaction' && entry.historyAvailable,
  );
  if (marker) marker.historyLoading = loading;
}

function applyHistoryTail(
  controller: SessionController,
  tail: ReturnType<typeof hydrateTranscript>,
): void {
  if (controller.historyLayers.length === 0) {
    if (controller.historyRequestId) {
      const marker = tail.find((entry) => entry.kind === 'compaction');
      if (marker) marker.historyLoading = true;
    }
    controller.historyPrefixLength = 0;
    controller.messages = tail;
    return;
  }

  const latestBoundary = tail.find((entry) => entry.kind === 'compaction');
  if (latestBoundary) {
    latestBoundary.historyAvailable = false;
    latestBoundary.historyLoading = false;
  }
  const prefix = historyPrefix(
    controller.historyLayers,
    controller.firstVisibleHistoryLayer,
  );
  controller.historyPrefixLength = prefix.length;
  controller.messages = [...prefix, ...tail];
}

function streamEntrySequence(entry: TranscriptEntry): number | undefined {
  const match =
    /^stream-(?:assistant|thinking|tool|skill|user|error)-(\d+)(?:-segment-\d+)*$/.exec(
      entry.id,
    );
  if (!match) return undefined;
  const sequence = Number(match[1]);
  return Number.isSafeInteger(sequence) ? sequence : undefined;
}

function sameLiveRow(
  hydrated: TranscriptEntry,
  live: TranscriptEntry,
): boolean {
  if (hydrated.kind !== live.kind) return false;
  if (hydrated.kind === 'tool') {
    return Boolean(
      hydrated.toolCallId && hydrated.toolCallId === live.toolCallId,
    );
  }
  if (hydrated.kind === 'skill') {
    return hydrated.skillName === live.skillName && hydrated.text === live.text;
  }
  if (hydrated.kind === 'assistant' || hydrated.kind === 'thinking') {
    return live.text.startsWith(hydrated.text);
  }
  return hydrated.text === live.text;
}

function liveStreamSuffix(
  previous: TranscriptEntry[],
  dispatchStreamSequence: number,
): TranscriptEntry[] {
  return previous.filter((entry) => {
    const sequence = streamEntrySequence(entry);
    return sequence !== undefined && sequence >= dispatchStreamSequence;
  });
}

function mergeLiveStreamSuffix(
  hydrated: TranscriptEntry[],
  previous: TranscriptEntry[],
  dispatchStreamSequence: number,
): TranscriptEntry[] {
  const live = liveStreamSuffix(previous, dispatchStreamSequence);
  if (live.length === 0) return hydrated;

  // Local rows can split one streamed text message into several visible rows.
  // Compare their combined text with Pi's single hydrated message, but retain
  // the split live rows so feedback can stay in the gap between them.
  const comparableLive: TranscriptEntry[] = [];
  for (const entry of live) {
    const last = comparableLive[comparableLive.length - 1];
    if (
      last &&
      (entry.kind === 'assistant' || entry.kind === 'thinking') &&
      last.kind === entry.kind &&
      (!last.source ||
        !entry.source ||
        (last.source.message === entry.source.message &&
          last.source.block === entry.source.block))
    ) {
      last.text += entry.text;
    } else {
      comparableLive.push({ ...entry });
    }
  }

  const comparableHydrated: { entry: TranscriptEntry; start: number }[] = [];
  hydrated.forEach((entry, start) => {
    const last = comparableHydrated[comparableHydrated.length - 1]?.entry;
    if (
      last &&
      (entry.kind === 'assistant' || entry.kind === 'thinking') &&
      last.kind === entry.kind &&
      last.source &&
      entry.source &&
      last.source.message === entry.source.message &&
      last.source.block === entry.source.block
    ) {
      last.text += entry.text;
    } else {
      comparableHydrated.push({ entry: { ...entry }, start });
    }
  });
  const maximumOverlap = Math.min(
    comparableHydrated.length,
    comparableLive.length,
  );
  for (let overlap = maximumOverlap; overlap > 0; overlap -= 1) {
    const hydratedStart = comparableHydrated.length - overlap;
    if (
      comparableLive
        .slice(0, overlap)
        .every((entry, index) =>
          sameLiveRow(comparableHydrated[hydratedStart + index]!.entry, entry),
        )
    ) {
      const completedTools = new Map(
        hydrated
          .filter(
            (entry) =>
              entry.kind === 'tool' &&
              entry.toolCallId &&
              entry.toolRunning === false,
          )
          .map((entry) => [entry.toolCallId, entry]),
      );
      return [
        ...hydrated.slice(0, comparableHydrated[hydratedStart]!.start),
        ...live.map((entry) => {
          const completed =
            entry.toolRunning && completedTools.get(entry.toolCallId);
          return completed
            ? {
                ...completed,
                id: entry.id,
                source: entry.source ?? completed.source,
              }
            : entry;
        }),
      ];
    }
  }
  return [...hydrated, ...live];
}

function mergeReconnectedStream(
  hydrated: TranscriptEntry[],
  previous: TranscriptEntry[],
): TranscriptEntry[] {
  const sourceKey = (entry: TranscriptEntry): string | undefined =>
    entry.source ? `${entry.source.message}:${entry.source.block}` : undefined;
  const persisted = new Map<string, string>();
  for (const entry of hydrated) {
    const key = sourceKey(entry);
    if (key) persisted.set(key, (persisted.get(key) ?? '') + entry.text);
  }
  const hydratedIds = new Set(hydrated.map((entry) => entry.id));
  const toolIds = new Set(
    hydrated.map((entry) => entry.toolCallId).filter(Boolean),
  );
  const live = liveStreamSuffix(previous, 0);
  const liveText = new Map<string, string>();
  for (const entry of live) {
    const key = sourceKey(entry);
    if (key) liveText.set(key, (liveText.get(key) ?? '') + entry.text);
  }
  const missing = live.filter((entry) => {
    if (entry.toolCallId && toolIds.has(entry.toolCallId)) return false;
    const key = sourceKey(entry);
    if (!key || !persisted.has(key)) return !hydratedIds.has(entry.id);
    const text = liveText.get(key)!;
    const authoritative = persisted.get(key)!;
    // Preserve genuinely unrecorded partial output, not content Pi refined.
    return text.length > authoritative.length && text.startsWith(authoritative);
  });
  // Matched source blocks must be extended in place. Text overlap with the
  // hydrated tail is not provenance: another assistant message may repeat it.
  const extensions = new Map<string, TranscriptEntry[]>();
  const appended: TranscriptEntry[] = [];
  for (const entry of missing) {
    const key = sourceKey(entry);
    if (!key || !persisted.has(key)) {
      appended.push(entry);
      continue;
    }
    const group = extensions.get(key) ?? [];
    group.push(entry);
    extensions.set(key, group);
  }
  const replaced = new Set<string>();
  return [
    ...hydrated.flatMap((entry) => {
      const key = sourceKey(entry);
      const extension = key ? extensions.get(key) : undefined;
      if (!key || !extension) return [entry];
      if (replaced.has(key)) return [];
      replaced.add(key);
      return extension;
    }),
    ...appended,
  ];
}

function reanchorCompactionLocalEntries(
  reconciled: TranscriptEntry[],
  previous: TranscriptEntry[],
  errors: LocalError[],
  dispatchStreamSequence: number,
): void {
  const live = liveStreamSuffix(previous, dispatchStreamSequence);
  const liveIds = new Set(live.map((entry) => entry.id));
  const errorsById = new Map(
    errors.map((error) => [localErrorId(error.key), error]),
  );
  const oldBase = previous.filter(
    (entry) =>
      entry.kind !== 'notice' &&
      !errorsById.has(entry.id) &&
      !liveIds.has(entry.id),
  ).length;
  const newBase = reconciled.length - live.length;
  let precedingLiveRows = 0;
  const observedLocalIds = new Set<string>();

  for (const entry of previous) {
    if (liveIds.has(entry.id)) {
      precedingLiveRows += 1;
      continue;
    }
    const error = errorsById.get(entry.id);
    if (entry.kind !== 'notice' && !error) continue;

    entry.anchor = newBase + precedingLiveRows;
    observedLocalIds.add(entry.id);
    if (error) error.anchor = entry.anchor;
  }

  for (const error of errors) {
    const id = localErrorId(error.key);
    if (observedLocalIds.has(id)) continue;
    const oldAnchor = error.anchor ?? oldBase + live.length;
    error.anchor = Math.max(0, oldAnchor + newBase - oldBase);
  }
}

function revealCachedHistory(controller: SessionController): void {
  if (controller.historyLayers.length === 0) return;
  controller.firstVisibleHistoryLayer = Math.max(
    0,
    controller.firstVisibleHistoryLayer - 1,
  );
  const tail = controller.messages.slice(controller.historyPrefixLength);
  applyHistoryTail(controller, tail);
}

/**
 * Capture a source boundary where possible, with a tail-relative row fallback.
 * Before bootstrap, an unknown predecessor settles once against loaded history.
 */
function anchorFields(controller: SessionController): {
  anchor?: number;
  boundary?: TranscriptSource;
  afterToolCallId?: string;
} {
  if (!controller.messagesLoaded) return {};
  const owned = controller.messages
    .slice(controller.historyPrefixLength)
    .filter(
      (message) => message.kind !== 'notice' && message.anchor === undefined,
    );
  const predecessor = owned[owned.length - 1];
  return {
    anchor: owned.length,
    ...(predecessor?.kind === 'tool' && predecessor.toolCallId
      ? { afterToolCallId: predecessor.toolCallId }
      : {}),
    ...(predecessor?.source
      ? {
          boundary: {
            ...predecessor.source,
            offset: predecessor.source.offset + predecessor.text.length,
          },
        }
      : {}),
  };
}

function pushError(
  controller: SessionController,
  text: string,
  label = 'Reply Failed',
): void {
  controller.messages.push({
    id: `stream-error-${controller.streamSequence++}`,
    kind: 'error',
    text,
    errorLabel: label,
  });
  if (!isControllerSelected(controller)) controller.unread = true;
}

function retryStatus(event: Record<string, unknown>): string {
  const failure = describePiError(stringValue(event.errorMessage));
  return retryPiErrorMessage(failure.kind);
}

function appendStream(
  controller: SessionController,
  kind: 'assistant' | 'thinking',
  delta: string,
  contentIndex?: number,
): void {
  if (!delta) return;
  controller.streamMessage ??= `live-${controller.streamSequence++}`;
  let predecessor: TranscriptEntry | undefined;
  for (
    let index = controller.messages.length - 1;
    index >= controller.historyPrefixLength;
    index--
  ) {
    const entry = controller.messages[index]!;
    if (entry.source?.message === controller.streamMessage) {
      predecessor = entry;
      break;
    }
    if (entry.kind !== 'notice' && entry.anchor === undefined) break;
  }
  const block =
    contentIndex ??
    (predecessor?.kind === kind
      ? predecessor.source!.block
      : (predecessor?.source?.block ?? -1) + 1);
  const source: TranscriptSource = {
    message: controller.streamMessage,
    block,
    offset: 0,
  };
  const last = controller.messages[controller.messages.length - 1];
  const lastSequence = last ? streamEntrySequence(last) : undefined;
  const belongsToReconciledRun =
    !controller.compactionReconciliationPending ||
    (lastSequence !== undefined &&
      lastSequence >= controller.compactionStreamSequence);
  if (
    last?.kind === kind &&
    last.id.startsWith('stream-') &&
    last.source?.message === source.message &&
    last.source.block === block &&
    belongsToReconciledRun
  ) {
    last.text += delta;
    return;
  }
  controller.messages.push({
    id: `stream-${kind}-${controller.streamSequence++}`,
    kind,
    text: delta,
    source: {
      ...source,
      offset:
        predecessor?.source?.block === block
          ? predecessor.source.offset + predecessor.text.length
          : 0,
    },
  });
}

export {
  resetHistory,
  setHistoryLoading,
  applyHistoryTail,
  mergeLiveStreamSuffix,
  mergeReconnectedStream,
  reanchorCompactionLocalEntries,
  revealCachedHistory,
  anchorFields,
  pushError,
  retryStatus,
  appendStream,
};
