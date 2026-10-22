import { describe, expect, it } from 'vitest';

import loadHighlighter, { type HighlightEngine } from './highlight-engine';
import { highlightHtml } from './highlight-render';

function sourceFrom(rendered: ReturnType<HighlightEngine['render']>): string {
  return rendered.lines
    .map((line) =>
      line.html.replace(/<[^>]*>/g, '').replace(
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
      ),
    )
    .join('');
}

const lexicalCases = [
  ['bash', 'if [ "$user" = "$(id)" ]; then echo "<unsafe>"; fi\n'],
  ['css', '.card:hover { color: var(--accent); }\n'],
  ['diff', '--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new\n'],
  ['go', 'package main\nfunc main() { println("hi") }\n'],
  ['html', '<script>const x = "<&";</script>\n'],
  ['javascript', 'const value = /a+/gi; // comment\n'],
  ['json', '{"name":"tau","enabled":true}\n'],
  ['markdown', '# heading\n\n`<code>`\n'],
  ['python', 'def hello(name: str):\n    return f"Hi {name}"\n'],
  ['rust', 'fn main() { println!("hi"); }\n'],
  ['toml', '[package]\nname = "tau"\n'],
  ['tsx', 'export const View = () => <main>{"<&"}</main>;\n'],
  ['typescript', 'const answer: number = 42; // comment\n'],
  [
    'vue',
    '<template><p>{{ user?.name }}</p></template>\n<script setup>const x = "<&"\n',
  ],
  ['yaml', 'title: "<&"\nitems:\n  - one\n'],
] as const;

describe('highlight engine', () => {
  it('loads and lexically renders every allowed grammar without changing source', async () => {
    const engine = await loadHighlighter();

    for (const [language, source] of lexicalCases) {
      const rendered = engine.render(source, language);
      const html = highlightHtml(rendered);

      expect(rendered.header).toContain(`class="language-${language}"`);
      expect(rendered.header).toContain('--tau-code-light:');
      expect(rendered.header).toContain('--tau-code-dark:');
      expect(sourceFrom(rendered)).toBe(source);
      expect(html).not.toContain('<script>');
    }
  });

  it('preserves real lexer offsets and matched comment styling', async () => {
    const engine = await loadHighlighter();
    for (const source of [
      '',
      '\n\n',
      'const\tx = "🦊";\r\n// café\rlone\n',
      '/* first\r\nsecond */\n',
      'no trailing newline',
    ]) {
      const rendered = engine.render(source, 'TS meta');
      expect(sourceFrom(rendered)).toBe(source);
      expect(rendered.lines.at(-1)?.offset).toBe(source.length);
      expect(rendered.header).toContain('class="language-ts"');
      expect(rendered.header).not.toContain('tabindex');
    }
    const html = highlightHtml(
      engine.render('/* first\nsecond */\nconst value = "text";', 'ts'),
    );
    expect(html).toContain('--tau-code-light:#adaeb1');
    expect(html).toContain('--tau-code-dark:#5a6673');
    expect(html).toContain('--tau-code-light-font-style:italic');
    expect(html).toContain('--tau-code-dark-font-style:italic');
    expect(html).toMatch(/font-style:italic[^"]*">second/);
  });

  it('bounds token markup for minified lines and declines non-HTML source', async () => {
    const engine = await loadHighlighter();
    const source = 'a+'.repeat(262_140);
    const rendered = engine.render(source, 'ts');
    expect(sourceFrom(rendered)).toBe(source);
    expect(highlightHtml(rendered).length).toBeLessThan(source.length + 1000);
    expect(() => engine.render('const value = "\0";', 'ts')).toThrow(
      'Non-HTML source',
    );
  });

  it('keeps incomplete and hostile Vue and shell fragments safe', async () => {
    const engine = await loadHighlighter();
    const cases = [
      ['vue', '<template><img src=x onerror=alert(1)>\n{{ unfinished'],
      ['bash', 'echo "$(<img src=x onerror=alert(1))\nif [ "$x" = '],
    ] as const;

    for (const [language, source] of cases) {
      const html = highlightHtml(engine.render(source, language));
      expect(html).toContain('&lt;');
      expect(html).not.toContain('<img');
      expect(html).not.toContain('<script>');
    }
  });
});
