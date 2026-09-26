import { parseDocument } from 'yaml';

interface FrontmatterField {
  key: string;
  value: string;
}

interface PreviewMarkdown {
  body: string;
  fields: FrontmatterField[];
}

const OPENING_FENCE = /^\uFEFF?---[ \t]*\r?\n/;
const CLOSING_FENCE = /^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)/m;

function formatValue(value: unknown, seen: Set<object>, depth: number): string {
  if (depth > 20) throw new Error('Frontmatter nesting limit');
  if (typeof value === 'string') return value.replace(/\s+/g, ' ').trim();
  if (value === null) return 'null';
  if (typeof value === 'number' || typeof value === 'boolean')
    return String(value);
  if (!Array.isArray(value) && !(value instanceof Map))
    throw new Error('Unsupported frontmatter value');
  if (seen.has(value)) throw new Error('Cyclic frontmatter');
  seen.add(value);
  let formatted: string;
  if (Array.isArray(value)) {
    formatted = value
      .map((item) => formatValue(item, seen, depth + 1))
      .join(value.every((item) => !(item instanceof Map)) ? ', ' : '; ');
    if (!value.length) formatted = '[]';
  } else {
    formatted = Array.from(value, ([key, item]) => {
      if (typeof key !== 'string')
        throw new Error('Unsupported frontmatter key');
      return `${key} · ${formatValue(item, seen, depth + 1)}`;
    }).join(', ');
    if (!value.size) formatted = '{}';
  }
  seen.delete(value);
  return formatted;
}

/** Only a complete, valid YAML mapping at the very start of a file is metadata. */
function parsePreviewMarkdown(source: string): PreviewMarkdown {
  const fallback = { body: source, fields: [] };
  const opening = OPENING_FENCE.exec(source);
  if (!opening) return fallback;
  const rest = source.slice(opening[0].length);
  const closing = CLOSING_FENCE.exec(rest);
  if (!closing) return fallback;

  try {
    const document = parseDocument(rest.slice(0, closing.index), {
      uniqueKeys: true,
    });
    if (document.errors.length) return fallback;
    const value: unknown = document.toJS({ mapAsMap: true });
    if (value === null && !rest.slice(0, closing.index).trim())
      return {
        body: rest.slice(closing.index + closing[0].length),
        fields: [],
      };
    if (!(value instanceof Map)) return fallback;
    const fields: FrontmatterField[] = Array.from(value, ([key, item]) => {
      if (typeof key !== 'string')
        throw new Error('Unsupported frontmatter key');
      return { key, value: formatValue(item, new Set(), 0) };
    });
    return { body: rest.slice(closing.index + closing[0].length), fields };
  } catch {
    return fallback;
  }
}

export { parsePreviewMarkdown };
export type { FrontmatterField, PreviewMarkdown };
