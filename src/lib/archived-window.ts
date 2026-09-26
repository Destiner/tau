import type { ArchivedGroup } from './archived-groups';

/** A global row budget across expanded groups; collapsed groups cost nothing. */
function archivedWindow<T>(
  groups: readonly ArchivedGroup<T>[],
  closed: ReadonlySet<string>,
  budget: number,
): { groups: ArchivedGroup<T>[]; hasMore: boolean } {
  const visible: ArchivedGroup<T>[] = [];
  let remaining = Math.max(0, budget);
  for (const group of groups) {
    if (closed.has(group.label)) {
      visible.push({ label: group.label, items: [] });
      continue;
    }
    if (remaining === 0) return { groups: visible, hasMore: true };
    const items = group.items.slice(0, remaining);
    visible.push({ label: group.label, items });
    remaining -= items.length;
    if (items.length < group.items.length) {
      return { groups: visible, hasMore: true };
    }
  }
  return { groups: visible, hasMore: false };
}

export default archivedWindow;
