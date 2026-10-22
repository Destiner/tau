import { renderMarkdown } from './markdown';
import type { TranscriptEntry } from './pi/transcript';

interface TextMatch {
  /** Offsets in the original projected string, in UTF-16 code units. */
  start: number;
  end: number;
}

interface SearchMatch extends TextMatch {
  entryId: string;
}

interface TranscriptIndexOptions {
  signal?: AbortSignal;
  batchSize?: number;
  /** Called between batches; defaults to yielding to the event loop. */
  yieldControl?: () => Promise<void>;
}

interface TextRun extends TextMatch {
  node: Text;
  from: number;
  to: number;
}

const BLOCKS = new Set([
  'ADDRESS',
  'ARTICLE',
  'ASIDE',
  'BLOCKQUOTE',
  'DD',
  'DETAILS',
  'DIV',
  'DL',
  'DT',
  'FIGCAPTION',
  'FIGURE',
  'FOOTER',
  'H1',
  'H2',
  'H3',
  'H4',
  'H5',
  'H6',
  'HEADER',
  'HR',
  'LI',
  'MAIN',
  'NAV',
  'OL',
  'P',
  'PRE',
  'SECTION',
  'SUMMARY',
  'TABLE',
  'TBODY',
  'TD',
  'TFOOT',
  'TH',
  'THEAD',
  'TR',
  'UL',
]);

const EXCLUDED = [
  'script',
  'style',
  'template',
  'noscript',
  'svg',
  'math',
  'img',
  'picture',
  'canvas',
  'video',
  'audio',
  'iframe',
  'object',
  'input',
  'textarea',
  'select',
  'button',
  '[hidden]',
  '[aria-hidden="true"]',
  '[data-tau-copy]',
  '[data-tau-expand]',
  '.diagram',
  // Diagrams render lazily; their temporary source must not enter the cache.
  '.code-block[data-tau-lang="mermaid" i]',
].join(',');

/** Shared by inert Markdown projection and live, rendered-content range lookup. */
function traverse(root: Node): { text: string; runs: TextRun[] } {
  let text = '';
  const runs: TextRun[] = [];
  let pending:
    { text: string; node?: Text; from?: number; to?: number } | undefined;

  function boundary(): void {
    if (text) pending = { text: '\n' };
  }

  function append(
    value: string,
    node?: Text,
    from = 0,
    to = from + value.length,
  ): void {
    if (!value) return;
    if (pending) {
      const separator = pending;
      pending = undefined;
      append(separator.text, separator.node, separator.from, separator.to);
    }
    const start = text.length;
    text += value;
    if (node) runs.push({ start, end: text.length, node, from, to });
  }

  function visit(node: Node, preformatted: boolean): void {
    if (node.nodeType === 3) {
      const value = node.textContent ?? '';
      if (preformatted) {
        append(value, node as Text);
        return;
      }
      // HTML layout collapses whitespace in prose, but not inside a pre.
      for (const token of value.matchAll(/[^\t\n\f\r ]+|[\t\n\f\r ]+/g)) {
        const from = token.index;
        if (/^[\t\n\f\r ]/.test(token[0])) {
          if (text && !pending) {
            pending = {
              text: ' ',
              node: node as Text,
              from,
              to: from + token[0].length,
            };
          }
        } else {
          append(token[0], node as Text, from);
        }
      }
      return;
    }
    if (node.nodeType === 1) {
      const element = node as HTMLElement;
      if (
        element.matches(EXCLUDED) ||
        element.style.display === 'none' ||
        element.style.visibility === 'hidden' ||
        element.style.visibility === 'collapse'
      )
        return;
      if (element.tagName === 'BR') {
        pending = undefined;
        append('\n');
        return;
      }
      const block = BLOCKS.has(element.tagName);
      if (block) boundary();
      for (const child of element.childNodes) {
        visit(child, preformatted || element.tagName === 'PRE');
      }
      if (block) boundary();
      return;
    }
    for (const child of node.childNodes) visit(child, preformatted);
  }

  visit(root, false);
  return { text, runs };
}

/** Browser-only. The template remains inert and is never attached to the document. */
function projectTranscriptText(source: string): string {
  const template = document.createElement('template');
  template.innerHTML = renderMarkdown(source);
  return traverse(template.content).text;
}

/**
 * root must be the Markdown content container, not the whole transcript row.
 * Synthetic block newlines have no DOM node; ranges span the surrounding text.
 * Out-of-bounds offsets and offsets splitting a surrogate pair are skipped.
 * Callers must pass offsets from the current source; same-length stale text is not detectable.
 */
function resolveTranscriptRanges(
  root: HTMLElement,
  matches: readonly TextMatch[],
): Range[] {
  const { text, runs } = traverse(root);
  const ranges: Range[] = [];
  for (const match of matches) {
    if (
      !Number.isInteger(match.start) ||
      !Number.isInteger(match.end) ||
      match.start < 0 ||
      match.end <= match.start ||
      match.end > text.length ||
      splitsSurrogate(text, match.start) ||
      splitsSurrogate(text, match.end)
    )
      continue;
    const first = runs[runBoundary(runs, match.start, 'end')];
    const last = runs[runBoundary(runs, match.end, 'start') - 1];
    if (!first || !last || first.start >= match.end || last.end <= match.start)
      continue;
    const range = root.ownerDocument.createRange();
    range.setStart(
      first.node,
      first.from + Math.max(0, match.start - first.start),
    );
    range.setEnd(
      last.node,
      match.end >= last.end ? last.to : last.from + match.end - last.start,
    );
    ranges.push(range);
  }
  return ranges;
}

function runBoundary(
  runs: readonly TextRun[],
  offset: number,
  edge: 'start' | 'end',
): number {
  let low = 0;
  let high = runs.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    const run = runs[middle]!;
    if (edge === 'end' ? run.end <= offset : run.start < offset)
      low = middle + 1;
    else high = middle;
  }
  return low;
}

function splitsSurrogate(text: string, offset: number): boolean {
  return (
    offset > 0 &&
    offset < text.length &&
    /[\uD800-\uDBFF]/.test(text[offset - 1]!) &&
    /[\uDC00-\uDFFF]/.test(text[offset]!)
  );
}

/** Literal, nonoverlapping Unicode simple-case-fold matches; no normalization or trimming. */
function findTranscriptMatches(text: string, query: string): TextMatch[] {
  if (!query.trim()) return [];
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp(escaped, 'giu');
  return Array.from(text.matchAll(pattern), (match) => ({
    start: match.index,
    end: match.index + match[0].length,
  })).filter(
    (match) =>
      !splitsSurrogate(text, match.start) && !splitsSurrogate(text, match.end),
  );
}

interface IndexedRow {
  source: string;
  text: string;
}

// A streaming entry can change source many times before a batch is published.
// Keep a small LRU per id so canceled work is reusable without retaining history.
const MAX_CACHED_SOURCES_PER_ENTRY = 2;

function searchableRows(
  entries: readonly TranscriptEntry[],
): { id: string; source: string }[] {
  return entries
    .filter((entry) => entry.kind === 'user' || entry.kind === 'assistant')
    .map(({ id, text }) => ({ id, source: text }));
}

/** Own one instance per session. Updates prune removed and no-longer-searchable rows. */
class TranscriptSearchIndex {
  private rows = new Map<string, IndexedRow>();
  /** Unpublished projections from canceled or superseded updates. */
  private readonly projectionCache = new Map<string, Map<string, IndexedRow>>();
  private generation = 0;

  private readonly project: (source: string) => string;

  constructor(project: (source: string) => string = projectTranscriptText) {
    this.project = project;
  }

  get size(): number {
    return this.rows.size;
  }

  clear(): void {
    this.generation++;
    this.rows.clear();
    this.projectionCache.clear();
  }

  private pruneProjectionCache(rows: readonly { id: string }[]): void {
    const ids = new Set(rows.map(({ id }) => id));
    for (const id of this.projectionCache.keys()) {
      if (!ids.has(id)) this.projectionCache.delete(id);
    }
  }

  private cachedRow(id: string, source: string): IndexedRow | undefined {
    const committed = this.rows.get(id);
    if (committed?.source === source) return committed;
    const sources = this.projectionCache.get(id);
    const cached = sources?.get(source);
    if (!cached || !sources) return undefined;
    // Refresh this source's LRU position.
    sources.delete(source);
    sources.set(source, cached);
    return cached;
  }

  private cacheProjection(id: string, row: IndexedRow): void {
    let sources = this.projectionCache.get(id);
    if (!sources) {
      sources = new Map();
      this.projectionCache.set(id, sources);
    }
    sources.delete(row.source);
    sources.set(row.source, row);
    if (sources.size > MAX_CACHED_SOURCES_PER_ENTRY)
      sources.delete(sources.keys().next().value!);
  }

  private rowFor(id: string, source: string): [IndexedRow, boolean] {
    const cached = this.cachedRow(id, source);
    if (cached) return [cached, false];
    return [{ source, text: this.project(source) }, true];
  }

  private publish(rows: Map<string, IndexedRow>): void {
    this.rows = rows;
    for (const [id, row] of rows) {
      const sources = this.projectionCache.get(id);
      if (!sources) continue;
      sources.delete(row.source);
      if (!sources.size) this.projectionCache.delete(id);
    }
  }

  update(entries: readonly TranscriptEntry[]): void {
    this.generation++;
    const rows = searchableRows(entries);
    this.pruneProjectionCache(rows);
    const next = new Map<string, IndexedRow>();
    for (const { id, source } of rows) next.set(id, this.rowFor(id, source)[0]);
    this.publish(next);
  }

  /** Atomic publication: cancellation/supersession leaves the previous index intact. */
  async updateAsync(
    entries: readonly TranscriptEntry[],
    options: TranscriptIndexOptions = {},
  ): Promise<boolean> {
    const generation = ++this.generation;
    const rows = searchableRows(entries);
    const next = new Map<string, IndexedRow>();
    const batchSize = options.batchSize ?? 32;
    if (!Number.isInteger(batchSize) || batchSize < 1)
      throw new RangeError('batchSize must be a positive integer');
    this.pruneProjectionCache(rows);
    const yieldControl =
      options.yieldControl ??
      ((): Promise<void> =>
        new Promise<void>((resolve) => setTimeout(resolve, 0)));
    const stale = (): boolean =>
      options.signal?.aborted === true || generation !== this.generation;
    let projected = 0;
    for (let i = 0; i < rows.length; i++) {
      if (stale()) return false;
      const { id, source } = rows[i]!;
      const [row, newlyProjected] = this.rowFor(id, source);
      next.set(id, row);
      if (newlyProjected) {
        this.cacheProjection(id, row);
        projected++;
      }
      if (newlyProjected && projected % batchSize === 0 && i + 1 < rows.length)
        await yieldControl();
    }
    if (stale()) return false;
    this.publish(next);
    return true;
  }

  search(query: string): SearchMatch[] {
    const matches: SearchMatch[] = [];
    if (!query.trim()) return matches;
    for (const [entryId, row] of this.rows) {
      for (const match of findTranscriptMatches(row.text, query))
        matches.push({ entryId, ...match });
    }
    return matches;
  }
}

export type { TextMatch, SearchMatch, TranscriptIndexOptions };
export {
  projectTranscriptText,
  resolveTranscriptRanges,
  findTranscriptMatches,
  TranscriptSearchIndex,
};
