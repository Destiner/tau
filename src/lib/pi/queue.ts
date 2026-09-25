type QueueKind = 'steer' | 'followUp';

interface QueueSnapshot {
  steering: string[];
  followUp: string[];
}

interface QueueSubmission {
  id: string;
  text: string;
  draft: string;
  kind: QueueKind;
  /** Snapshot version when dispatched; a newer snapshot may precede its RPC reply. */
  version: number;
  baselineCount: number;
}

function emptyQueue(): QueueSnapshot {
  return { steering: [], followUp: [] };
}

function parseQueueSnapshot(
  value: Record<string, unknown>,
): QueueSnapshot | undefined {
  if (!Array.isArray(value.steering) || !Array.isArray(value.followUp)) return;
  if (
    !value.steering.every((item) => typeof item === 'string') ||
    !value.followUp.every((item) => typeof item === 'string')
  )
    return;
  return { steering: [...value.steering], followUp: [...value.followUp] };
}

/** Text is not an identity. Match occurrences only to avoid showing a pre-ack
 * optimistic item twice when an event beats its response. */
function visibleQueue(
  snapshot: QueueSnapshot,
  pending: QueueSubmission[],
): QueueSnapshot {
  const result = {
    steering: [...snapshot.steering],
    followUp: [...snapshot.followUp],
  };
  const counted = {
    steering: new Map<string, number>(),
    followUp: new Map<string, number>(),
  };
  for (const submission of pending) {
    const key = submission.kind === 'steer' ? 'steering' : 'followUp';
    const occurrences = counted[key].get(submission.text) ?? 0;
    counted[key].set(submission.text, occurrences + 1);
    const present = result[key].filter(
      (text) => text === submission.text,
    ).length;
    if (present <= submission.baselineCount + occurrences)
      result[key].push(submission.text);
  }
  return result;
}

function queueHasWork(
  snapshot: QueueSnapshot,
  pending: QueueSubmission[],
): boolean {
  return (
    snapshot.steering.length + snapshot.followUp.length + pending.length > 0
  );
}

export { emptyQueue, parseQueueSnapshot, queueHasWork, visibleQueue };
export type { QueueKind, QueueSnapshot, QueueSubmission };
