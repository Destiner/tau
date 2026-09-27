import { describe, expect, it } from 'vitest';

import { applyHeldOrder, heldSessions } from './session-order';

function sessions(...ids: string[]): { id: string }[] {
  return ids.map((id) => ({ id }));
}

describe('applyHeldOrder', () => {
  it('keeps held order despite re-sorting and closes gaps', () => {
    const held = heldSessions(sessions('a', 'b', 'c'));

    expect(applyHeldOrder(sessions('c', 'a', 'b'), held)).toEqual(
      sessions('a', 'b', 'c'),
    );
    expect(applyHeldOrder(sessions('c', 'a'), held)).toEqual(
      sessions('a', 'c'),
    );
  });

  it('sorts freely without a hold', () => {
    expect(applyHeldOrder(sessions('c', 'a', 'b'), undefined)).toEqual(
      sessions('c', 'a', 'b'),
    );
  });

  it('hides new sessions during a hold, even for single or empty projects', () => {
    const held = heldSessions(sessions('a', 'b'));

    expect(applyHeldOrder(sessions('new', 'b', 'a'), held)).toEqual(
      sessions('a', 'b'),
    );
    expect(
      applyHeldOrder(sessions('new', 'a'), heldSessions(sessions('a'))),
    ).toEqual(sessions('a'));
    expect(applyHeldOrder(sessions('new'), [])).toEqual([]);
  });

  it('retains the materialized row across an ID change and registration', () => {
    const visible = sessions('phantom', 'other');
    const held = heldSessions(visible);
    visible[0]!.id = 'materialized';
    const current = [{ id: 'other', status: 'idle' }, visible[0]!];

    expect(applyHeldOrder(current, held)).toEqual([
      { id: 'materialized' },
      { id: 'other', status: 'idle' },
    ]);
    const registered = { id: 'materialized', status: 'working' };

    expect(applyHeldOrder([registered, visible[1]!], held)).toEqual([
      registered,
      visible[1],
    ]);
  });

  it('keeps the outgoing phase when its ephemeral row becomes a replacement', () => {
    const visible = sessions('plan', 'other');
    const held = heldSessions(visible);
    visible[0]!.id = 'implement';
    const current = [
      visible[0]!,
      { id: 'plan', status: 'idle' },
      { id: 'other', status: 'idle' },
    ];

    expect(applyHeldOrder(current, held)).toEqual([
      { id: 'plan', status: 'idle' },
      { id: 'other', status: 'idle' },
    ]);
  });
});
