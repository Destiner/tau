import { chromium, type Browser, type Page } from '@playwright/test';
import { createServer, type ViteDevServer } from 'vite';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import type { TranscriptEntry } from './pi/transcript';
import {
  findTranscriptMatches,
  TranscriptSearchIndex,
} from './transcript-search';

type SearchModule = typeof import('./transcript-search');
declare global {
  interface Window {
    transcriptSearchTest: SearchModule;
    transcriptMarkdownTest: typeof import('./markdown');
  }
}

const row = (
  id: string,
  text: string,
  kind: TranscriptEntry['kind'] = 'assistant',
): TranscriptEntry => ({ id, text, kind });

describe('literal transcript matching', () => {
  it('ignores empty and whitespace-only queries, without trimming meaningful queries', () => {
    for (const query of ['', ' \n\t', '\u00a0'])
      expect(findTranscriptMatches('text', query)).toEqual([]);
    expect(findTranscriptMatches(' x x ', ' x ')).toEqual([
      { start: 0, end: 3 },
    ]);
  });

  it('is case-insensitive, literal and nonoverlapping', () => {
    expect(findTranscriptMatches('aAaAa', 'aa')).toEqual([
      { start: 0, end: 2 },
      { start: 2, end: 4 },
    ]);
    const literal = '.*+?^${}()|[]\\';
    expect(findTranscriptMatches(`before ${literal} after`, literal)).toEqual([
      { start: 7, end: 7 + literal.length },
    ]);
  });

  it('preserves original UTF-16 offsets across emoji and Unicode case folding', () => {
    const text = '😀İ K Σςσ 𐐀𐐨 end';
    expect(findTranscriptMatches(text, 'k')).toEqual([{ start: 4, end: 5 }]);
    expect(findTranscriptMatches(text, 'σ')).toEqual([
      { start: 6, end: 7 },
      { start: 7, end: 8 },
      { start: 8, end: 9 },
    ]);
    expect(findTranscriptMatches(text, '𐐨')).toEqual([
      { start: 10, end: 12 },
      { start: 12, end: 14 },
    ]);
    expect(findTranscriptMatches(text, 'end')).toEqual([
      { start: 15, end: 18 },
    ]);
    expect(findTranscriptMatches(text, '\ud83d')).toEqual([]);
    expect(findTranscriptMatches('e\u0301', 'é')).toEqual([]);
    expect(findTranscriptMatches('ß', 'ss')).toEqual([]);
  });
});

describe('transcript index', () => {
  it('includes only user and assistant text, preserving row and occurrence order', () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    const excluded: TranscriptEntry['kind'][] = [
      'thinking',
      'tool',
      'skill',
      'error',
      'notice',
      'compaction',
    ];
    index.update([
      row('u', 'hit hit', 'user'),
      ...excluded.map((kind) => row(kind, 'hit', kind)),
      row('a', 'HIT'),
    ]);
    expect(project).toHaveBeenCalledTimes(2);
    expect(index.search('hit')).toEqual([
      { entryId: 'u', start: 0, end: 3 },
      { entryId: 'u', start: 4, end: 7 },
      { entryId: 'a', start: 0, end: 3 },
    ]);
    expect(index.search('  ')).toEqual([]);
  });

  it('caches by identity and source, handles in-place streaming and prunes removed rows', () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    const a = row('a', 'old');
    const b = row('b', 'keep');
    index.update([a, b]);
    index.update([{ ...b }, { ...a }]);
    index.search('keep');
    index.search('old');
    expect(project).toHaveBeenCalledTimes(2);
    a.text = 'new';
    index.update([a, b]);
    expect(project).toHaveBeenCalledTimes(3);
    expect(index.search('old')).toEqual([]);
    expect(index.search('new')).toEqual([{ entryId: 'a', start: 0, end: 3 }]);
    index.update([row('a', 'new', 'thinking')]);
    expect(index.size).toBe(0);
    index.update([b]);
    expect(project).toHaveBeenCalledTimes(4);
    index.clear();
    expect(index.size).toBe(0);
    expect(index.search('keep')).toEqual([]);
  });

  it('publishes batches atomically, snapshots mutable input and reuses cached rows', async () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    index.update([row('a', 'old')]);
    const entries = [row('a', 'old'), row('b', 'new')];
    const yieldControl = vi.fn(async () => {
      expect(index.size).toBe(1);
      entries[1]!.text = 'mutated';
    });
    expect(
      await index.updateAsync(entries, { batchSize: 1, yieldControl }),
    ).toBe(true);
    expect(yieldControl).not.toHaveBeenCalled();
    expect(project).toHaveBeenCalledTimes(2);
    expect(index.search('new')).toEqual([{ entryId: 'b', start: 0, end: 3 }]);
    expect(await index.updateAsync([])).toBe(true);
    expect(index.size).toBe(0);
  });

  it('yields only once when a new batch precedes warm rows', async () => {
    const index = new TranscriptSearchIndex((source) => source);
    const warm = Array.from({ length: 100 }, (_, id) =>
      row(`warm-${id}`, 'warm'),
    );
    index.update(warm);
    const added = Array.from({ length: 32 }, (_, id) =>
      row(`new-${id}`, 'new'),
    );
    const yieldControl = vi.fn(async () => undefined);
    await index.updateAsync([...added, ...warm], { yieldControl });
    expect(yieldControl).toHaveBeenCalledTimes(1);
    expect(index.size).toBe(132);
  });

  it('does not yield a warm update', async () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    const entries = Array.from({ length: 100 }, (_, id) =>
      row(`${id}`, 'warm'),
    );
    index.update(entries);
    const yieldControl = vi.fn(async () => undefined);

    await expect(
      index.updateAsync(entries, { batchSize: 1, yieldControl }),
    ).resolves.toBe(true);
    expect(project).toHaveBeenCalledTimes(100);
    expect(yieldControl).not.toHaveBeenCalled();
  });

  it('reuses superseded projections without publishing a partial index', async () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    index.update([row('published', 'published')]);
    const releases: (() => void)[] = [];
    const yieldControl = vi.fn(
      () => new Promise<void>((resolve) => releases.push(resolve)),
    );
    const entries = [row('a', 'a'), row('b', 'b'), row('c', 'c')];

    const superseded = index.updateAsync(entries, {
      batchSize: 1,
      yieldControl,
    });
    const replacement = index.updateAsync(entries, {
      batchSize: 1,
      yieldControl,
    });
    expect(project).toHaveBeenCalledTimes(3); // published, then a and b
    expect(index.search('a')).toEqual([]);
    expect(index.search('published')).toHaveLength(1);

    releases[0]!();
    releases[1]!();
    await expect(superseded).resolves.toBe(false);
    await expect(replacement).resolves.toBe(true);
    expect(index.search('a')).toEqual([{ entryId: 'a', start: 0, end: 1 }]);
  });

  it('bounds unpublished source caches and prunes them with removed rows and clear', async () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    const cacheCanceledSource = async (source: string): Promise<void> => {
      const controller = new AbortController();
      await expect(
        index.updateAsync([row('same', source), row('tail', 'tail')], {
          signal: controller.signal,
          batchSize: 1,
          yieldControl: async () => controller.abort(),
        }),
      ).resolves.toBe(false);
    };

    await cacheCanceledSource('a');
    await cacheCanceledSource('b');
    await cacheCanceledSource('c');
    expect(project).toHaveBeenCalledTimes(3);
    index.update([row('same', 'a')]);
    expect(project).toHaveBeenCalledTimes(4); // only b and c were retained

    index.update([row('other', 'other')]);
    index.update([row('same', 'b')]);
    expect(project).toHaveBeenCalledTimes(6); // removed ids are not retained
    index.clear();
    index.update([row('same', 'b')]);
    expect(project).toHaveBeenCalledTimes(7);
  });

  it('rejects aborted, superseded and cleared work without resurrecting stale rows', async () => {
    for (const action of ['abort', 'sync', 'async', 'clear'] as const) {
      const index = new TranscriptSearchIndex((source) => source);
      const controller = new AbortController();
      index.update([row('original', 'original')]);
      const result = await index.updateAsync(
        [row('x', 'stale'), row('y', 'stale')],
        {
          signal: controller.signal,
          batchSize: 1,
          yieldControl: async () => {
            if (action === 'abort') controller.abort();
            if (action === 'sync') index.update([row('fresh', 'fresh')]);
            if (action === 'async')
              await index.updateAsync([row('fresh', 'fresh')]);
            if (action === 'clear') index.clear();
          },
        },
      );
      expect(result).toBe(false);
      expect(index.search('stale')).toEqual([]);
      expect(index.size).toBe(action === 'clear' ? 0 : 1);
      if (action === 'sync' || action === 'async')
        expect(index.search('fresh')).toHaveLength(1);
    }
  });

  it('keeps row identity separate from identical source and follows reordered rows', () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    index.update([row('a', 'same'), row('b', 'same')]);
    expect(project).toHaveBeenCalledTimes(2);
    index.update([row('b', 'same'), row('a', 'same')]);
    expect(project).toHaveBeenCalledTimes(2);
    expect(index.search('same').map((match) => match.entryId)).toEqual([
      'b',
      'a',
    ]);
  });

  it('preserves the published index on projection failure', async () => {
    const index = new TranscriptSearchIndex((source) => {
      if (source === 'bad') throw new Error('projection failed');
      return source;
    });
    index.update([row('original', 'keep')]);
    expect(() => index.update([row('new', 'new'), row('bad', 'bad')])).toThrow(
      'projection failed',
    );
    await expect(index.updateAsync([row('bad', 'bad')])).rejects.toThrow(
      'projection failed',
    );
    expect(index.search('keep')).toEqual([
      { entryId: 'original', start: 0, end: 4 },
    ]);
  });

  it('yields by default and observes cancellation before the next batch', async () => {
    const index = new TranscriptSearchIndex((source) => source);
    const controller = new AbortController();
    const pending = index.updateAsync([row('a', 'a'), row('b', 'b')], {
      batchSize: 1,
      signal: controller.signal,
    });
    controller.abort();
    expect(await pending).toBe(false);
    expect(index.size).toBe(0);
  });

  it('does not parse pre-aborted work and validates batch sizes', async () => {
    const project = vi.fn((source: string) => source);
    const index = new TranscriptSearchIndex(project);
    const controller = new AbortController();
    controller.abort();
    expect(
      await index.updateAsync([row('a', 'x')], { signal: controller.signal }),
    ).toBe(false);
    expect(project).not.toHaveBeenCalled();
    for (const batchSize of [0, -1, 1.5, Infinity, NaN]) {
      await expect(index.updateAsync([], { batchSize })).rejects.toThrow(
        RangeError,
      );
    }
  });
});

// Use the existing browser dependency: no simulated DOM or replacement Markdown parser.
describe('rendered projection and DOM ranges', () => {
  let server: ViteDevServer;
  let browser: Browser;
  let page: Page;

  beforeAll(async () => {
    server = await createServer({
      configFile: false,
      server: { port: 0, host: '127.0.0.1' },
    });
    await server.listen();
    browser = await chromium.launch({ headless: true });
    page = await browser.newPage();
    const origin = server.resolvedUrls!.local[0]!;
    await page.route(origin, (route) =>
      route.fulfill({ contentType: 'text/html', body: '<html></html>' }),
    );
    await page.goto(origin);
    await page.addScriptTag({
      type: 'module',
      content: `
      import * as search from '/src/lib/transcript-search.ts';
      import * as markdown from '/src/lib/markdown.ts';
      window.transcriptSearchTest = search;
      window.transcriptMarkdownTest = markdown;
    `,
    });
    await page.waitForFunction(() => !!window.transcriptSearchTest);
  }, 30_000);

  afterAll(async () => {
    await browser?.close();
    await server?.close();
  });

  async function project(source: string): Promise<string> {
    return page.evaluate(
      (source) => window.transcriptSearchTest.projectTranscriptText(source),
      source,
    );
  }

  it('uses visible prose, entities, inline code, link labels and preformatted code', async () => {
    expect(
      await project(
        '# Title\n\nHello **bold** and *italic* &amp; `inline`.\n\n[Label](https://example.com/secret)\n\n```ts\nconst value = 1;\n```',
      ),
    ).toBe('Title\nHello bold and italic & inline.\nLabel\nconst value = 1;\n');
  });

  it('separates paragraphs, lists, tables, blockquotes and hard breaks without joining blocks', async () => {
    expect(
      await project(
        'first\n\nsecond\n\n- one\n- two\n\n> quote\n\n| A | B |\n| - | - |\n| C | D |\n\nlast\nline',
      ),
    ).toBe('first\nsecond\none\ntwo\nquote\nA\nB\nC\nD\nlast\nline');
    expect(await project('<div>left</div><div>right</div>')).toBe(
      'left\nright',
    );
    expect(await project('hel**lo**')).toBe('hello');
    expect(await project('a   <em>b</em>   c')).toBe('a b c');
  });

  it('excludes diagrams, images, generated controls and hidden or non-content markup', async () => {
    const source =
      'visible\n\n![secret alt](https://example.com/secret.png)\n\n' +
      '<span hidden>hidden</span><span aria-hidden="true">aria</span>' +
      '<span style="display:none">display</span><span style="visibility:hidden">visibility</span>' +
      '<button>control</button><script>unsafe</script><style>bad</style>' +
      '<svg><text>svg label</text></svg><div class="diagram">diagram label</div>\n\n' +
      '```mermaid\ngraph TD\n A[Secret diagram] --> B[Other]\n```\n\nend';
    expect(await project(source)).toBe('visible\nend');
    expect(await project('')).toBe('');
    expect(await project('![alt](image.png)')).toBe('');
  });

  it('resolves cross-inline matches and file labels identically in actual rendered DOM', async () => {
    const result = await page.evaluate(() => {
      const api = window.transcriptSearchTest;
      const { renderMarkdown } = window.transcriptMarkdownTest;
      const source =
        '😀 Hello **Wor**ld & [link](https://example.com) `src/App.vue`\n\n```ts\nconst value = 1;\n```';
      const text = api.projectTranscriptText(source);
      const root = document.createElement('div');
      root.innerHTML = renderMarkdown(source, { copyPaths: true });
      root.querySelector('[data-tau-copy]')!.textContent = 'Generated Copy';
      const ranges = api.resolveTranscriptRanges(root, [
        ...api.findTranscriptMatches(text, 'hello world & link'),
        ...api.findTranscriptMatches(text, 'src/App.vue'),
        ...api.findTranscriptMatches(text, 'const value'),
      ]);
      return ranges.map((range) => range.toString());
    });
    expect(result).toEqual([
      'Hello World & link',
      'src/App.vue',
      'const value',
    ]);
  });

  it('ignores invalid ranges and preserves original whitespace and astral character endpoints', async () => {
    const result = await page.evaluate(() => {
      const api = window.transcriptSearchTest;
      const root = document.createElement('div');
      root.innerHTML = '<p>😀 a   <em>b</em></p><p>c</p>';
      const valid = api.resolveTranscriptRanges(root, [
        { start: 0, end: 6 },
        { start: 4, end: 5 },
        { start: 6, end: 8 },
      ]);
      const invalid = api.resolveTranscriptRanges(root, [
        { start: -1, end: 1 },
        { start: 0, end: 99 },
        { start: 2, end: 2 },
        { start: 1, end: 2 },
        { start: 0, end: 1 },
        { start: 0.5, end: 2 },
        { start: NaN, end: 2 },
        { start: 6, end: 7 },
      ]);
      return {
        valid: valid.map((range) => range.toString()),
        invalid: invalid.length,
      };
    });
    expect(result).toEqual({ valid: ['😀 a   b', '   ', 'c'], invalid: 0 });
  });
});
