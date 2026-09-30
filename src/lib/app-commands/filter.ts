interface FuzzyFilterItem {
  title: string;
  keywords?: readonly string[];
}

/** Returns subsequence matches while preserving the provider's original order. */
function filterFuzzy<T extends FuzzyFilterItem>(
  items: readonly T[],
  query: string,
): T[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...items];
  return items.filter((item) => {
    const haystacks = [item.title, ...(item.keywords ?? [])].map((value) =>
      value.toLocaleLowerCase(),
    );
    return terms.every((term) =>
      haystacks.some((value) => fuzzyIncludes(value, term)),
    );
  });
}

function fuzzyIncludes(value: string, query: string): boolean {
  let at = 0;
  for (const character of value) {
    if (character === query[at]) at += 1;
    if (at === query.length) return true;
  }
  return query.length === 0;
}

/** Palette pages use this for the revised Backspace navigation rule. */
function paletteBackspaceReturnsRoot(
  key: string,
  query: string,
  page: 'root' | string,
): boolean {
  return key === 'Backspace' && query.length === 0 && page !== 'root';
}

export {
  filterFuzzy,
  fuzzyIncludes,
  paletteBackspaceReturnsRoot,
  type FuzzyFilterItem,
};
