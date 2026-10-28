import { afterEach, describe, expect, it, vi } from 'vitest';

import type { HighlightEngine } from './highlight-engine';

function engine(
  render = vi.fn<HighlightEngine['render']>((code) => ({
    header: '<pre>',
    lines: [{ html: code, offset: code.length }],
  })),
): HighlightEngine {
  return { render } as unknown as HighlightEngine;
}

async function loadWith(
  loader: () => Promise<HighlightEngine>,
): Promise<(typeof import('./highlight'))['default']> {
  vi.doMock('./highlight-engine', () => ({ default: loader }));
  const module = await import('./highlight');
  return module.default;
}

afterEach(() => {
  vi.doUnmock('./highlight-engine');
  vi.resetModules();
  vi.restoreAllMocks();
});

describe('highlighting a fenced block', () => {
  it('treats a failed import or tokenizer failure as unavailable', async () => {
    const importError = new Error('missing wasm');
    const importLog = vi
      .spyOn(console, 'error')
      .mockImplementation(() => undefined);
    const unavailable = await loadWith(async () => Promise.reject(importError));
    await vi.dynamicImportSettled();

    expect(unavailable('const value = 1', 'ts')).toBeNull();
    expect(importLog).toHaveBeenCalledWith(
      'Could not load the syntax highlighter',
    );

    vi.resetModules();
    vi.doUnmock('./highlight-engine');
    const renderLog = vi
      .spyOn(console, 'error')
      .mockImplementation(() => undefined);
    const failing = await loadWith(async () =>
      engine(
        vi.fn(() => {
          throw new Error('bad tokens');
        }),
      ),
    );
    await vi.dynamicImportSettled();

    expect(failing('const value = 1', 'ts')).toBeNull();
    expect(renderLog).toHaveBeenCalledWith(
      'Could not highlight the code block',
    );
  });

  it('rejects unsupported and oversized blocks before tokenizing', async () => {
    const render = vi.fn(engine().render);
    const highlightCode = await loadWith(async () => engine(render));
    await vi.dynamicImportSettled();

    expect(highlightCode('let a = 1', 'swift')).toBeNull();
    expect(highlightCode('plain text', '')).toBeNull();
    expect(highlightCode('const value = "\0";', 'ts')).toBeNull();
    expect(highlightCode('x'.repeat(20_000), 'ts')).not.toBeNull();
    expect(highlightCode('x'.repeat(20_001), 'ts')).toBeNull();
    expect(render).toHaveBeenCalledOnce();
  });
});
