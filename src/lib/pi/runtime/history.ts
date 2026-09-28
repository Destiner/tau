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
    /^stream-(?:assistant|thinking|tool|skill|user|error)-(\d+)$/.exec(
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
      last.kind === entry.kind
    ) {
      last.text += entry.text;
    } else {
      comparableLive.push({ ...entry });
    }
  }

  const maximumOverlap = Math.min(hydrated.length, comparableLive.length);
  for (let overlap = maximumOverlap; overlap > 0; overlap -= 1) {
    const hydratedStart = hydrated.length - overlap;
    if (
      comparableLive
        .slice(0, overlap)
        .every((entry, index) =>
          sameLiveRow(hydrated[hydratedStart + index]!, entry),
        )
    ) {
      return [...hydrated.slice(0, hydratedStart), ...live];
    }
  }
  return [...hydrated, ...live];
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
 * The spot a local entry has to hold on to so a rebuild from Pi's messages puts
 * it back where it was: the number of transcript rows Pi owns right now, zero
 * of them included. A session whose transcript has not loaded yet cannot tell a
 * session without history from one whose history has yet to arrive, and an entry
 * anchored above history it actually followed would be worse than one left at
 * the bottom, so only that case claims no spot at all.
 */
function anchorFields(controller: SessionController): { anchor?: number } {
  if (!controller.messagesLoaded) return {};
  return {
    anchor: controller.messages.filter(
      (message) => message.kind !== 'notice' && message.anchor === undefined,
    ).length,
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
): void {
  if (!delta) return;
  const last = controller.messages[controller.messages.length - 1];
  const lastSequence = last ? streamEntrySequence(last) : undefined;
  const belongsToReconciledRun =
    !controller.compactionReconciliationPending ||
    (lastSequence !== undefined &&
      lastSequence >= controller.compactionStreamSequence);
  if (
    last?.kind === kind &&
    last.id.startsWith('stream-') &&
    belongsToReconciledRun
  ) {
    last.text += delta;
    return;
  }
  controller.messages.push({
    id: `stream-${kind}-${controller.streamSequence++}`,
    kind,
    text: delta,
  });
}

export {
  resetHistory,
  setHistoryLoading,
  applyHistoryTail,
  mergeLiveStreamSuffix,
  reanchorCompactionLocalEntries,
  revealCachedHistory,
  anchorFields,
  pushError,
  retryStatus,
  appendStream,
};
