import type { VirtualItem } from '@tanstack/vue-virtual';
import { describe, expect, it } from 'vitest';

import {
  POSITION_LIMIT,
  recallScroll,
  rememberScroll,
} from './transcript-scroll';

/*
 * The store is a module-level map by design — it outlives every transcript it
 * serves — so each test works under keys of its own rather than resetting it.
 */

function measurements(size: number): VirtualItem[] {
  return [{ index: 0, key: 'row-0', start: 0, size, end: size, lane: 0 }];
}

describe('transcript scroll positions', () => {
  it('gives a session back the position it was left at', () => {
    rememberScroll('recall', {
      offset: 1280,
      following: false,
      measurements: measurements(64),
    });

    expect(recallScroll('recall')).toEqual({
      offset: 1280,
      following: false,
      measurements: measurements(64),
    });
  });

  it('has nothing for a session that has not been read', () => {
    expect(recallScroll('unread')).toBeUndefined();
  });

  it('keeps the latest position for a session', () => {
    rememberScroll('latest', {
      offset: 10,
      following: false,
      measurements: [],
    });
    rememberScroll('latest', {
      offset: 20,
      following: true,
      measurements: [],
    });

    expect(recallScroll('latest')?.offset).toBe(20);
    expect(recallScroll('latest')?.following).toBe(true);
  });

  it('keeps no position for a session without a key', () => {
    rememberScroll('', { offset: 40, following: false, measurements: [] });

    expect(recallScroll('')).toBeUndefined();
  });

  it('drops the least recently left session once the limit is passed', () => {
    const key = (index: number): string => `evicted-${index}`;
    for (let index = 0; index <= POSITION_LIMIT; index += 1) {
      rememberScroll(key(index), {
        offset: index,
        following: false,
        measurements: [],
      });
    }

    expect(recallScroll(key(0))).toBeUndefined();
    expect(recallScroll(key(1))?.offset).toBe(1);
    expect(recallScroll(key(POSITION_LIMIT))?.offset).toBe(POSITION_LIMIT);
  });

  it('counts leaving a session again as recent use', () => {
    const key = (index: number): string => `refreshed-${index}`;
    rememberScroll(key(0), { offset: 0, following: false, measurements: [] });
    for (let index = 1; index < POSITION_LIMIT; index += 1) {
      rememberScroll(key(index), {
        offset: index,
        following: false,
        measurements: [],
      });
    }
    rememberScroll(key(0), { offset: 99, following: false, measurements: [] });
    rememberScroll(key(POSITION_LIMIT), {
      offset: POSITION_LIMIT,
      following: false,
      measurements: [],
    });

    expect(recallScroll(key(0))?.offset).toBe(99);
    expect(recallScroll(key(1))).toBeUndefined();
  });
});
