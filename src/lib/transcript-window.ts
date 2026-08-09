import type { TranscriptEntry } from "../types";

export const transcriptWindowSize = 80;
export const transcriptWindowStep = 40;
const transcriptTextLimit = 100_000;

export function latestWindowStart(messages: TranscriptEntry[]): number {
  let start = messages.length;
  let textLength = 0;
  while (start > 0 && messages.length - start < transcriptWindowSize) {
    const nextLength = textLength + messages[start - 1].text.length;
    if (start < messages.length && nextLength > transcriptTextLimit) break;
    textLength = nextLength;
    start -= 1;
  }
  return start;
}

export function transcriptWindowEnd(
  messages: TranscriptEntry[],
  start: number,
): number {
  let end = Math.min(Math.max(start, 0), messages.length);
  let textLength = 0;
  while (end < messages.length && end - start < transcriptWindowSize) {
    const nextLength = textLength + messages[end].text.length;
    if (end > start && nextLength > transcriptTextLimit) break;
    textLength = nextLength;
    end += 1;
  }
  return end;
}

export function olderWindowStart(start: number): number {
  return Math.max(0, start - transcriptWindowStep);
}

export function newerWindowStart(
  start: number,
  messages: TranscriptEntry[],
): number {
  return Math.min(latestWindowStart(messages), start + transcriptWindowStep);
}
