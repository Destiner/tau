import type { VirtualItem } from '@tanstack/vue-virtual';

/**
 * How many sessions keep a position before the least recently left is dropped.
 * Positions are cheap, but a long run opening hundreds of sessions should not
 * hold every measurement it has ever taken.
 */
const POSITION_LIMIT = 50;

interface TranscriptScrollPosition {
  /** Scroll offset in pixels, meaningful only alongside `measurements`. */
  offset: number;
  /**
   * Whether the reader was at the end. It outranks the offset on the way back:
   * a session goes on streaming while it is off screen, so the end they were
   * sitting at is no longer the pixel it was.
   */
  following: boolean;
  /**
   * The virtualizer's measured row sizes, by row key. Rows are estimated until
   * they are measured, so an offset alone points at different content on a
   * fresh mount unless the measurements come back with it.
   */
  measurements: VirtualItem[];
}

/**
 * Where each session's transcript was left, for the lifetime of the app. This
 * is view state rather than session state: nothing renders from it, it is read
 * once as a transcript mounts, so it stays a plain map outside reactivity.
 */
const positions = new Map<string, TranscriptScrollPosition>();

function rememberScroll(key: string, position: TranscriptScrollPosition): void {
  if (!key) return;
  // Re-inserting keeps the map ordered least recently left first.
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
