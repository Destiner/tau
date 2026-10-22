import { afterEach, describe, expect, it, vi } from 'vitest';

import type { HighlightEngine } from './highlight-engine';

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

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
  it('waits for readiness without attempting a render early', async () => {
    const pending = deferred<HighlightEngine>();
    const render = vi.fn(engine().render);
    const highlightCode = await loadWith(() => pending.promise);

    expect(highlightCode('const before = true', 'ts')).toBeNull();
    expect(render).not.toHaveBeenCalled();

    pending.resolve(engine(render));
    await pending.promise;
    await vi.dynamicImportSettled();

    expect(highlightCode('const after = true', 'ts')).toBe(
      '<pre>const after = true</code></pre>',
    );
    expect(render).toHaveBeenCalledOnce();
  });

  it('keeps aliases as distinct cache keys and evicts the FIFO entry at 200 blocks', async () => {
    const render = vi.fn(engine().render);
    const highlightCode = await loadWith(async () => engine(render));
    await vi.dynamicImportSettled();

    expect(highlightCode('const same = true', 'ts')).not.toBeNull();
    expect(highlightCode('const same = true', 'ts')).not.toBeNull();
    expect(highlightCode('const same = true', 'typescript')).not.toBeNull();
    expect(render).toHaveBeenCalledTimes(2);

    for (let index = 0; index < 198; index += 1) {
      expect(highlightCode(`const cache${index} = true`, 'ts')).not.toBeNull();
    }
    expect(render).toHaveBeenCalledTimes(200);

    expect(highlightCode('const newest = true', 'ts')).not.toBeNull();
    expect(render).toHaveBeenCalledTimes(201);
    expect(highlightCode('const same = true', 'ts')).not.toBeNull();
    expect(render).toHaveBeenCalledTimes(202);
  });

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
