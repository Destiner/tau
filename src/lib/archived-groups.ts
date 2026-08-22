/*
 * Time bucketing for the flat archived-sessions list. Buckets follow calendar
 * boundaries where the name promises one (days, Monday-based weeks); anything
 * older than last week is "Older", regardless of month or year. Pure and
 * injectable with `now` so it can be tested deterministically.
 */

const ARCHIVED_GROUP_LABELS = [
  'Today',
  'Yesterday',
  'This Week',
  'Last Week',
  'Older',
] as const;

type ArchivedGroupLabel = (typeof ARCHIVED_GROUP_LABELS)[number];

const DAY_MS = 86_400_000;
const WEEK_MS = 7 * DAY_MS;

function startOfDay(timestamp: number): number {
  const date = new Date(timestamp);
  return new Date(
    date.getFullYear(),
    date.getMonth(),
    date.getDate(),
  ).getTime();
}

/** Monday-based start of the week containing `timestamp`. */
function startOfWeek(timestamp: number): number {
  const date = new Date(startOfDay(timestamp));
  date.setDate(date.getDate() - ((date.getDay() + 6) % 7));
  return date.getTime();
}

function archivedGroupLabelFor(
  timestamp: number,
  now: number = Date.now(),
): ArchivedGroupLabel {
  if (timestamp <= 0) return 'Older';
  const day = Math.round((startOfDay(now) - startOfDay(timestamp)) / DAY_MS);
  if (day <= 0) return 'Today';
  if (day === 1) return 'Yesterday';
  const week = Math.floor(
    (startOfWeek(now) - startOfWeek(timestamp)) / WEEK_MS,
  );
  if (week <= 0) return 'This Week';
  if (week === 1) return 'Last Week';
  return 'Older';
}

interface ArchivedGroup<T> {
  label: ArchivedGroupLabel;
  items: T[];
}

/**
 * Groups `items` into the fixed label order by `timestamp`, dropping empty
 * groups. Items inside a group keep their incoming (newest-first) order.
 */
function groupArchivedByTime<T>(
  items: readonly T[],
  timestamp: (item: T) => number,
  now: number = Date.now(),
): ArchivedGroup<T>[] {
  const buckets = new Map<ArchivedGroupLabel, T[]>();
  for (const item of items) {
    const label = archivedGroupLabelFor(timestamp(item), now);
    const bucket = buckets.get(label);
    if (bucket) bucket.push(item);
    else buckets.set(label, [item]);
  }
  return ARCHIVED_GROUP_LABELS.filter((label) => buckets.has(label)).map(
    (label) => ({ label, items: buckets.get(label) ?? [] }),
  );
}

export { ARCHIVED_GROUP_LABELS, archivedGroupLabelFor, groupArchivedByTime };

export type { ArchivedGroup, ArchivedGroupLabel };
