import { describe, expect, it } from 'vitest';

import {
  ARCHIVED_GROUP_LABELS,
  archivedGroupLabelFor,
  groupArchivedByTime,
} from './archived-groups';

const DAY = 86_400_000;
// A Monday, so week arithmetic in tests reads plainly.
const NOW = new Date(2026, 7, 24, 15, 0, 0).getTime();

describe('archivedGroupLabelFor', () => {
  it('buckets by calendar day for today and yesterday', () => {
    expect(archivedGroupLabelFor(NOW - 1, NOW)).toBe('Today');
    // 23:59 yesterday is still yesterday even though it is over 24h ago... and
    // 00:01 today before noon is less than 24h ago but not "yesterday".
    const yesterdayEvening = new Date(2026, 7, 23, 23, 59, 0).getTime();
    const earlyToday = new Date(2026, 7, 24, 0, 1, 0).getTime();
    expect(archivedGroupLabelFor(yesterdayEvening, NOW)).toBe('Yesterday');
    expect(archivedGroupLabelFor(earlyToday, NOW)).toBe('Today');
  });

  it('buckets by Monday-based calendar week', () => {
    // From a Thursday, Tuesday of the same week is "This Week", while the
    // preceding calendar week is "Last Week" and anything before it "Older".
    const thursday = new Date(2026, 7, 27, 15, 0, 0).getTime();
    const tuesday = new Date(2026, 7, 25, 12, 0, 0).getTime();
    const lastMonday = new Date(2026, 7, 17, 12, 0, 0).getTime();
    expect(archivedGroupLabelFor(tuesday, thursday)).toBe('This Week');
    expect(archivedGroupLabelFor(lastMonday, thursday)).toBe('Last Week');
    expect(archivedGroupLabelFor(lastMonday - DAY, thursday)).toBe('Older');
    expect(archivedGroupLabelFor(lastMonday - 8 * DAY, thursday)).toBe('Older');
  });

  it('folds everything older than last week into Older', () => {
    expect(archivedGroupLabelFor(NOW - 20 * DAY, NOW)).toBe('Older');
    expect(archivedGroupLabelFor(NOW - 400 * DAY, NOW)).toBe('Older');
  });

  it('sends unknown timestamps to Older', () => {
    expect(archivedGroupLabelFor(0, NOW)).toBe('Older');
  });
});

describe('groupArchivedByTime', () => {
  it('keeps the fixed label order and drops empty groups', () => {
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
  });

  it('preserves incoming order within a group', () => {
    const groups = groupArchivedByTime(
      [
        { id: 'newest', at: NOW - 1_000 },
        { id: 'middle', at: NOW - 2_000 },
        { id: 'oldest', at: NOW - 3_000 },
      ],
      (item) => item.at,
      NOW,
    );
    expect(groups).toHaveLength(1);
    expect(groups[0]?.label).toBe('Today');
    expect(groups[0]?.items.map((item) => item.id)).toEqual([
      'newest',
      'middle',
      'oldest',
    ]);
  });

  it('exposes every label for callers that need the full order', () => {
    expect(ARCHIVED_GROUP_LABELS).toEqual([
      'Today',
      'Yesterday',
      'This Week',
      'Last Week',
      'Older',
    ]);
  });
});
