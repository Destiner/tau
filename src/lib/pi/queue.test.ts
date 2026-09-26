import { describe, expect, it } from 'vitest';

import {
  emptyQueue,
  parseQueueSnapshot,
  queueHasWork,
  visibleQueue,
  type QueueSubmission,
} from './queue';

const submission = (
  id: string,
  kind: 'steer' | 'followUp',
  text: string,
  baselineCount = 0,
): QueueSubmission => ({
  id,
  kind,
  text,
  draft: text,
  version: 0,
  baselineCount,
});

describe('Pi queue projection', () => {
  it('accepts only complete text snapshots and keeps repeated occurrences', () => {
    expect(
      parseQueueSnapshot({ steering: ['same', 'same'], followUp: ['same'] }),
    ).toEqual({ steering: ['same', 'same'], followUp: ['same'] });
    expect(
      parseQueueSnapshot({ steering: ['ok'], followUp: [2] }),
    ).toBeUndefined();
    expect(parseQueueSnapshot({ steering: ['ok'] })).toBeUndefined();
  });
  it('shows each in-flight submission once even when snapshots beat acknowledgements', () => {
    const pending = [
      submission('1', 'steer', 'same'),
      submission('2', 'steer', 'same'),
      submission('3', 'followUp', 'same'),
    ];
    expect(visibleQueue(emptyQueue(), pending)).toEqual({
      steering: ['same', 'same'],
      followUp: ['same'],
    });
    expect(
      visibleQueue({ steering: ['same'], followUp: ['same'] }, pending),
    ).toEqual({ steering: ['same', 'same'], followUp: ['same'] });
    expect(
      visibleQueue({ steering: ['same', 'same'], followUp: ['same'] }, pending),
    ).toEqual({ steering: ['same', 'same'], followUp: ['same'] });
  });
  it('preserves an existing identical entry when a new one is in flight', () => {
    const pending = [submission('new', 'steer', 'same', 1)];
    expect(
      visibleQueue({ steering: ['same'], followUp: [] }, pending).steering,
    ).toEqual(['same', 'same']);
    expect(queueHasWork(emptyQueue(), pending)).toBe(true);
    expect(queueHasWork(emptyQueue(), [])).toBe(false);
  });
});
