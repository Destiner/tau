import { describe, expect, it, vi } from 'vitest';

import highlightCode from './highlight';

/** A grammar is loaded asynchronously, so nothing highlights on the first tick. */
async function ready(): Promise<void> {
  await vi.waitFor(() => {
    expect(highlightCode('const ready = true;\n', 'ts')).not.toBeNull();
  });
}

/** Milliseconds spent on a piece of work, whatever it reports. */
function elapsed(work: () => void): number {
  const start = performance.now();
  work();
  return performance.now() - start;
}

describe('highlighting a fenced block', () => {
  it('marks up a language it holds a grammar for', async () => {
    await ready();
    const html = highlightCode('const a: number = 1; // note\n', 'ts');

    // Both schemes at once, as custom properties the stylesheet chooses between.
    expect(html).toContain('--tau-code-light:');
    expect(html).toContain('--tau-code-dark:');
    // The comment is a scope of its own, which is what a broken engine loses.
    expect(html).toMatch(
      /--tau-code-light-font-style:italic[^"]*">\s*\/\/ note/,
    );
  });

  it('names the language as the fence did, in the class marked would write', async () => {
    await ready();

    expect(highlightCode('echo hi\n', 'console')).toContain(
      'class="language-console"',
    );
    expect(highlightCode('const a = 1;\n', 'TS meta here')).toContain(
      'class="language-ts"',
    );
  });

  it('leaves the transcript its own tab order', async () => {
    await ready();

    expect(highlightCode('a: 1\n', 'yaml')).not.toContain('tabindex');
  });

  it('reports a block it will not highlight', async () => {
    await ready();

    expect(highlightCode('let a = 1\n', 'swift')).toBeNull();
    expect(highlightCode('plain text\n', '')).toBeNull();
    // Long enough that highlighting it would be felt in a streaming answer.
    expect(highlightCode('const a = 1;\n'.repeat(2_000), 'ts')).toBeNull();
  });

  it('holds a block it has already highlighted', async () => {
    await ready();

    // Long enough to measure, and never highlighted before, so the first pass
    // pays for the block itself rather than for loading the grammar.
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
