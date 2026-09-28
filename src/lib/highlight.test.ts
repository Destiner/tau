import { describe, expect, it, vi } from 'vitest';

import highlightCode from './highlight';

async function ready(): Promise<void> {
  await vi.waitFor(() => {
    expect(highlightCode('const ready = true;\n', 'ts')).not.toBeNull();
  });
}

function elapsed(work: () => void): number {
  const start = performance.now();
  work();
  return performance.now() - start;
}

describe('highlighting a fenced block', () => {
  it('highlights known fences with theme styles and safe markup', async () => {
    await ready();
    const html = highlightCode('const a: number = 1; // note\n', 'ts');

    expect(html).toContain('--tau-code-light:');
    expect(html).toContain('--tau-code-dark:');

    expect(html).toMatch(
      /--tau-code-light-font-style:italic[^"]*">\s*\/\/ note/,
    );
    expect(highlightCode('echo hi\n', 'console')).toContain(
      'class="language-console"',
    );
    expect(highlightCode('const a = 1;\n', 'TS meta here')).toContain(
      'class="language-ts"',
    );
    expect(highlightCode('a: 1\n', 'yaml')).not.toContain('tabindex');
  });

  it('reports a block it will not highlight', async () => {
    await ready();

    expect(highlightCode('let a = 1\n', 'swift')).toBeNull();
    expect(highlightCode('plain text\n', '')).toBeNull();

    expect(highlightCode('x'.repeat(20_000), 'ts')).not.toBeNull();
    expect(highlightCode('x'.repeat(20_001), 'ts')).toBeNull();
  });

  it('holds a block it has already highlighted', async () => {
    await ready();

    // Long enough to measure, and never highlighted before, so the first pass

    const code = 'export const value: number = 1; // comment\n'.repeat(400);
    const first = elapsed(() => {
      highlightCode(code, 'ts');
    });
    const repeated = elapsed(() => {
      for (let pass = 0; pass < 20; pass += 1) highlightCode(code, 'ts');
    });

    expect(repeated).toBeLessThan(first);
  });
});
