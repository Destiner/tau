import type { VirtualItem } from '@tanstack/vue-virtual';

const POSITION_LIMIT = 50;

interface TranscriptScrollPosition {
  offset: number;

  following: boolean;
  /**
   * The virtualizer's measured row sizes, by row key. Rows are estimated until
   * they are measured, so an offset alone points at different content on a
   * fresh mount unless the measurements come back with it.
   */
  measurements: VirtualItem[];
}

const positions = new Map<string, TranscriptScrollPosition>();

function rememberScroll(key: string, position: TranscriptScrollPosition): void {
  if (!key) return;

  positions.delete(key);
  positions.set(key, position);

  for (const oldest of positions.keys()) {
    if (positions.size <= POSITION_LIMIT) break;
    positions.delete(oldest);
  }
}

function recallScroll(key: string): TranscriptScrollPosition | undefined {
  return key ? positions.get(key) : undefined;
}

export {
  POSITION_LIMIT,
  type TranscriptScrollPosition,
  recallScroll,
  rememberScroll,
};
