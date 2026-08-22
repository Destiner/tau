import { describe, expect, it } from 'vitest';

import { applyHeldOrder, heldSessionIds } from './session-order';

function sessions(...ids: string[]): { id: string }[] {
  return ids.map((id) => ({ id }));
}

describe('applyHeldOrder', () => {
  it('keeps the held order when activity re-sorts the list', () => {
    const held = heldSessionIds(sessions('a', 'b', 'c'));

    expect(applyHeldOrder(sessions('c', 'a', 'b'), held)).toEqual(
      sessions('a', 'b', 'c'),
    );
  });

  it('sorts freely without a hold', () => {
    expect(applyHeldOrder(sessions('c', 'a', 'b'), [])).toEqual(
      sessions('c', 'a', 'b'),
    );
  });

  it('closes the gap a session leaving the list opens', () => {
    const held = heldSessionIds(sessions('a', 'b', 'c'));

    expect(applyHeldOrder(sessions('c', 'a'), held)).toEqual(
      sessions('a', 'c'),
    );
  });

  it('leaves a session that appeared while held where the sort put it', () => {
    const held = heldSessionIds(sessions('a', 'b'));

    expect(applyHeldOrder(sessions('new', 'b', 'a'), held)).toEqual(
      sessions('new', 'a', 'b'),
    );
  });

  it('holds nothing when a single session is left to hold', () => {
    const held = heldSessionIds(sessions('a'));

    expect(applyHeldOrder(sessions('new', 'a'), held)).toEqual(
      sessions('new', 'a'),
    );
  });
});
