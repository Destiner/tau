import { describe, expect, it } from 'vitest';

import {
  ARCHIVED_GROUP_LABELS,
  archivedGroupLabelFor,
  groupArchivedByTime,
} from './archived-groups';

const DAY = 86_400_000;

const NOW = new Date(2026, 7, 24, 15, 0, 0).getTime();

describe('archivedGroupLabelFor', () => {
  it('buckets by calendar day and Monday-based week, with unknown dates older', () => {
    expect(archivedGroupLabelFor(NOW - 1, NOW)).toBe('Today');

    const yesterdayEvening = new Date(2026, 7, 23, 23, 59, 0).getTime();
    const earlyToday = new Date(2026, 7, 24, 0, 1, 0).getTime();
    expect(archivedGroupLabelFor(yesterdayEvening, NOW)).toBe('Yesterday');
    expect(archivedGroupLabelFor(earlyToday, NOW)).toBe('Today');

    const thursday = new Date(2026, 7, 27, 15, 0, 0).getTime();
    const tuesday = new Date(2026, 7, 25, 12, 0, 0).getTime();
    const lastMonday = new Date(2026, 7, 17, 12, 0, 0).getTime();
    expect(archivedGroupLabelFor(tuesday, thursday)).toBe('This Week');
    expect(archivedGroupLabelFor(lastMonday, thursday)).toBe('Last Week');
    expect(archivedGroupLabelFor(lastMonday - DAY, thursday)).toBe('Older');
    expect(archivedGroupLabelFor(lastMonday - 8 * DAY, thursday)).toBe('Older');
    expect(archivedGroupLabelFor(NOW - 20 * DAY, NOW)).toBe('Older');
    expect(archivedGroupLabelFor(NOW - 400 * DAY, NOW)).toBe('Older');
    expect(archivedGroupLabelFor(0, NOW)).toBe('Older');
  });
});

describe('groupArchivedByTime', () => {
  it('orders nonempty groups and preserves incoming item order', () => {
    const groups = groupArchivedByTime(
      [
        { id: 'old', at: NOW - 30 * DAY },
        { id: 'today', at: NOW - 1_000 },
        { id: 'last-week', at: NOW - 4 * DAY },
      ],
      (item) => item.at,
      NOW,
    );
    expect(groups.map((group) => group.label)).toEqual([
      'Today',
      'Last Week',
      'Older',
    ]);
    expect(groups[0]?.items.map((item) => item.id)).toEqual(['today']);
    expect(ARCHIVED_GROUP_LABELS).toEqual([
      'Today',
      'Yesterday',
      'This Week',
      'Last Week',
      'Older',
    ]);
    const sameDay = groupArchivedByTime(
      [
        { id: 'newest', at: NOW - 1_000 },
        { id: 'middle', at: NOW - 2_000 },
        { id: 'oldest', at: NOW - 3_000 },
      ],
      (item) => item.at,
      NOW,
    );
    expect(sameDay).toHaveLength(1);
    expect(sameDay[0]?.label).toBe('Today');
    expect(sameDay[0]?.items.map((item) => item.id)).toEqual([
      'newest',
      'middle',
      'oldest',
    ]);
  });
});
