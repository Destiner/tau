import type { TokensResult } from '@pierre/highlights';
import { describe, expect, it } from 'vitest';

import { highlightHtml, renderHighlight } from './highlight-render';

function decodeHtml(value: string): string {
  return value.replace(/<[^>]*>/g, '').replace(
    /&(amp|lt|gt|quot|#39|#13);/g,
    (_match, entity: string) =>
      ({
        amp: '&',
        lt: '<',
        gt: '>',
        quot: '"',
        '#39': "'",
        '#13': '\r',
      })[entity]!,
  );
}

function tokensFor(source: string): TokensResult {
  let offset = 0;
  return {
    tokens: source.split('\n').map((line) => {
      const token = line
        ? [{ content: line, offset, htmlStyle: { color: '#abc' } }]
        : [];
      offset += line.length + 1;
      return token;
    }),
  };
}

describe('highlight rendering', () => {
  it.each([
    '',
    'one',
    'one\n',
    'one\ntwo',
    'one\r\ntwo\r\n',
    'one\rtwo',
    '😀 café\n漢字\n',
    '\n\ntrailing\n',
    `${'x'.repeat(25_000)}\n${'😀'.repeat(2_000)}`,
  ])(
    'reconstructs the exact UTF-16 source and line offsets for %j',
    (source) => {
      const rendered = renderHighlight(source, tokensFor(source), 'typescript');

      expect(rendered.lines.map((line) => decodeHtml(line.html)).join('')).toBe(
        source,
      );
      expect(rendered.lines.at(-1)?.offset).toBe(source.length);
      expect(rendered.lines.map((line) => line.offset)).toEqual(
        [...source.matchAll(/\n/g)]
          .map((match) => match.index! + 1)
          .concat(source.length),
      );
    },
  );

  it('escapes root, tag, gaps, and token styles while preserving complete styles', () => {
    const source = `<script>& "'\r\n`;
    const rendered = renderHighlight(
      source,
      {
        rootStyle: 'color:red;--unsafe:<&"\'\r',
        tokens: [
          [
            {
              content: '<script>',
              offset: 0,
              htmlStyle: {
                '--tau-code-light': '#fff',
                'font-style': 'italic',
                color: 'url("<&")',
              },
            },
          ],
          [],
        ],
      },
      'ts" onmouseover="x',
    );
    const html = highlightHtml(rendered);

    expect(rendered.header).toBe(
      '<pre class="tau-highlight" data-tau-highlight style="color:red;--unsafe:&lt;&amp;&quot;&#39;&#13;"><code class="language-ts&quot; onmouseover=&quot;x">',
    );
    expect(html).toContain(
      '<span style="--tau-code-light:#fff;font-style:italic;color:url(&quot;&lt;&amp;&quot;)">&lt;script&gt;</span>',
    );
    expect(html).toContain('&amp; &quot;&#39;&#13;</span>\n');
    expect(decodeHtml(rendered.lines.map((line) => line.html).join(''))).toBe(
      source,
    );
  });

  it('rejects lexer output that cannot account for the source exactly', () => {
    expect(() => renderHighlight('a', { tokens: [] }, 'ts')).toThrow(
      'Invalid highlight lines',
    );
    expect(() =>
      renderHighlight('a', { tokens: [[{ content: 'b', offset: 0 }]] }, 'ts'),
    ).toThrow('Invalid highlight offsets');
    expect(() =>
      renderHighlight('a', { tokens: [[{ content: 'a', offset: 1 }]] }, 'ts'),
    ).toThrow('Invalid highlight offsets');
  });
});
