import { describe, expect, test } from 'vitest';

import type { ArchivedGroup } from './archived-groups';
import archivedWindow from './archived-window';

const groups: ArchivedGroup<number>[] = [
  { label: 'Today', items: Array.from({ length: 75 }, (_, i) => i) },
  { label: 'Yesterday', items: Array.from({ length: 26 }, (_, i) => i + 75) },
];
const empty = new Set<string>();
const ids = (result: ReturnType<typeof archivedWindow<number>>): number[] =>
  result.groups.flatMap((group) => group.items);

describe('archived window', () => {
  test.each([0, 1, 49, 50, 51, 99, 100, 101])('budget %i', (budget) => {
    const result = archivedWindow(groups, empty, budget);
    expect(ids(result)).toEqual(Array.from({ length: budget }, (_, i) => i));
    expect(result.hasMore).toBe(budget < 101);
  });

  test('skips collapsed rows without consuming the global budget', () => {
    const closed = new Set(['Today']);
    const result = archivedWindow(groups, closed, 10);
    expect(result.groups.map((group) => group.label)).toEqual([
      'Today',
      'Yesterday',
    ]);
    expect(ids(result)).toEqual(Array.from({ length: 10 }, (_, i) => i + 75));
    expect(
      archivedWindow(groups, new Set(['Today', 'Yesterday']), 50).hasMore,
    ).toBe(false);
  });

  test('does not show a heading for an expanded group beyond the budget', () => {
    expect(
      archivedWindow(groups, empty, 50).groups.map((group) => group.label),
    ).toEqual(['Today']);
    expect(ids(archivedWindow(groups, empty, 100))).toEqual(
      Array.from({ length: 100 }, (_, i) => i),
    );
    expect(ids(archivedWindow(groups, empty, 150))).toEqual(
      Array.from({ length: 101 }, (_, i) => i),
    );
  });

  test('refills after removal or replacement without stale object references', () => {
    const updated = groups.map((group) => ({
      ...group,
      items: group.items.filter((i) => i !== 0).map((i) => i + 1000),
    }));
    expect(ids(archivedWindow(updated, empty, 50))).toEqual(
      Array.from({ length: 50 }, (_, i) => i + 1001),
    );
    expect(archivedWindow([], empty, 50)).toEqual({
      groups: [],
      hasMore: false,
    });
  });
});
