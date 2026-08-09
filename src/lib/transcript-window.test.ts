import { describe, expect, it } from "vitest";
import type { TranscriptEntry } from "../types";
import {
  latestWindowStart,
  newerWindowStart,
  olderWindowStart,
  transcriptWindowEnd,
  transcriptWindowSize,
} from "./transcript-window";

function messages(count: number): TranscriptEntry[] {
  return Array.from({ length: count }, (_, index) => ({
    id: String(index),
    kind: "assistant",
    text: `Message ${index}`,
  }));
}

describe("transcript window", () => {
  it("mounts only the latest bounded slice", () => {
    const transcript = messages(200);
    const start = latestWindowStart(transcript);
    expect(start).toBe(200 - transcriptWindowSize);
    expect(transcriptWindowEnd(transcript, start)).toBe(200);
  });

  it("moves in overlapping steps", () => {
    const transcript = messages(200);
    expect(olderWindowStart(120)).toBe(80);
    expect(newerWindowStart(80, transcript)).toBe(120);
  });

  it("keeps one oversized entry visible", () => {
    const transcript = messages(2);
    transcript[1].text = "x".repeat(120_000);
    expect(latestWindowStart(transcript)).toBe(1);
    expect(transcriptWindowEnd(transcript, 1)).toBe(2);
  });
});
