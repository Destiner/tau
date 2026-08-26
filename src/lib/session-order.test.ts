import { describe, expect, it } from 'vitest';

import { applyHeldOrder, heldSessions } from './session-order';

function sessions(...ids: string[]): { id: string }[] {
  return ids.map((id) => ({ id }));
}

describe('applyHeldOrder', () => {
  it('keeps the held order when activity re-sorts the list', () => {
    const held = heldSessions(sessions('a', 'b', 'c'));

    expect(applyHeldOrder(sessions('c', 'a', 'b'), held)).toEqual(
      sessions('a', 'b', 'c'),
    );
  });

  it('sorts freely without a hold', () => {
    expect(applyHeldOrder(sessions('c', 'a', 'b'), undefined)).toEqual(
      sessions('c', 'a', 'b'),
    );
  });

  it('closes the gap a session leaving the list opens', () => {
    const held = heldSessions(sessions('a', 'b', 'c'));

    expect(applyHeldOrder(sessions('c', 'a'), held)).toEqual(
      sessions('a', 'c'),
    );
  });

  it('hides a session that appeared during the hold', () => {
    const held = heldSessions(sessions('a', 'b'));

    expect(applyHeldOrder(sessions('new', 'b', 'a'), held)).toEqual(
      sessions('a', 'b'),
    );
  });

  it('holds a single-session project stable', () => {
    const held = heldSessions(sessions('a'));

    expect(applyHeldOrder(sessions('new', 'a'), held)).toEqual(sessions('a'));
  });

  it('keeps a project that was empty on entry empty', () => {
    expect(applyHeldOrder(sessions('new'), [])).toEqual([]);
  });

  it('keeps a phantom row when its id changes during materialization', () => {
    const visible = sessions('phantom', 'other');
    const held = heldSessions(visible);
    visible[0]!.id = 'materialized';
    const current = [
      { id: 'other', status: 'idle' },
      { id: 'materialized', status: 'working' },
    ];

    expect(applyHeldOrder(current, held)).toEqual([
      { id: 'materialized', status: 'working' },
      { id: 'other', status: 'idle' },
    ]);
  });
});
