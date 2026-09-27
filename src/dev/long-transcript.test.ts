import { describe, expect, it } from 'vitest';

import createLongTranscript, {
  createPreviewTranscript,
  createShowcaseTranscript,
} from './long-transcript';

describe('showcase transcript', () => {
  it('matches the original thirteen tail entries and default transcript length', () => {
    const originalTail = createLongTranscript().slice(4_987);
    const showcase = createShowcaseTranscript();

    expect(createLongTranscript()).toHaveLength(5_000);
    expect(showcase).toHaveLength(13);
    expect(showcase).toEqual(originalTail);
    expect(showcase.map(({ id }) => id)).toEqual([
      'fixture-markdown-showcase-thinking',
      'fixture-notice-4988',
      'fixture-notice-4989',
      'fixture-notice-4990',
      'fixture-thinking-trace',
      'fixture-tool-done',
      'fixture-tool-failed',
      'fixture-tool-running',
      'fixture-thinking-sections',
      'fixture-markdown-showcase-user',
      'fixture-markdown-showcase',
      'fixture-skill-4998',
      'fixture-short-user',
    ]);
  });

  it('returns independent showcase and preview entries on every call', () => {
    const first = createShowcaseTranscript();
    const second = createShowcaseTranscript();
    expect(first).not.toBe(second);
    expect(first[0]).not.toBe(second[0]);
    first[0]!.text = 'changed';
    expect(second).toEqual(createLongTranscript().slice(4_987));

    const firstPreview = createPreviewTranscript();
    const secondPreview = createPreviewTranscript();
    expect(firstPreview).not.toBe(secondPreview);
    expect(firstPreview[0]).not.toBe(secondPreview[0]);
    firstPreview[0]!.text = 'changed';
    expect(secondPreview).toEqual(createShowcaseTranscript().slice(10, 11));
  });

  it('uses the original assistant showcase entry for preview', () => {
    const preview = createPreviewTranscript();
    expect(preview).toHaveLength(1);
    expect(preview).toEqual(createShowcaseTranscript().slice(10, 11));
    expect(preview[0]).toEqual(createLongTranscript()[4_997]);
    expect(preview[0]?.id).toBe('fixture-markdown-showcase');
  });
});
