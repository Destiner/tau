import { afterEach, describe, expect, it, vi } from 'vitest';

import loadHighlighter, { type HighlightEngine } from './highlight-engine';

interface WorkerRequest {
  source: string;
  language: string;
}

interface WorkerScope {
  onmessage?: (event: MessageEvent<WorkerRequest>) => Promise<void>;
  postMessage: (message: unknown) => void;
}

function createEngine(render: HighlightEngine['render']): HighlightEngine {
  return { render };
}

async function loadWorker(loader: () => Promise<HighlightEngine>): Promise<{
  messages: unknown[];
  send: (source: string, language: string) => Promise<void>;
}> {
  const messages: unknown[] = [];
  const scope: WorkerScope = {
    postMessage: vi.fn((message: unknown): void => {
      messages.push(message);
    }),
  };
  vi.resetModules();
  vi.stubGlobal('self', scope);
  vi.doMock('./highlight-engine', () => ({ default: loader }));
  await import('./preview-highlight.worker');

  return {
    messages,
    send: async (source: string, language: string): Promise<void> => {
      await scope.onmessage!({
        data: { source, language },
      } as MessageEvent<WorkerRequest>);
    },
  };
}

afterEach(() => {
  vi.doUnmock('./highlight-engine');
  vi.resetModules();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('preview highlight worker', () => {
  it('renders the complete source once, then starts, streams, and finishes', async () => {
    const source = 'const first = 1;\nconst second = 2;';
    const render = vi.fn<HighlightEngine['render']>((received, language) => ({
      header: `<pre data-language="${language}"><code>`,
      lines: [
        { html: '<span>const first = 1;</span>\n', offset: 17 },
        { html: '<span>const second = 2;</span>', offset: received.length },
      ],
    }));
    const { messages, send } = await loadWorker(async () =>
      createEngine(render),
    );

    await send(source, 'typescript');

    expect(render).toHaveBeenCalledOnce();
    expect(render).toHaveBeenCalledWith(source, 'typescript');
    expect(messages).toEqual([
      { type: 'start', header: '<pre data-language="typescript"><code>' },
      {
        type: 'chunk',
        html: '<span>const first = 1;</span>\n<span>const second = 2;</span>',
        offset: source.length,
      },
      { type: 'done' },
    ]);
  });

  it('limits chunks to 250 rendered lines', async () => {
    const lines = Array.from({ length: 251 }, (_, index) => ({
      html: `<span>${index}</span>`,
      offset: index + 1,
    }));
    const { messages, send } = await loadWorker(async () =>
      createEngine(vi.fn(() => ({ header: '<pre>', lines }))),
    );

    await send('source', 'typescript');

    expect(messages).toEqual([
      { type: 'start', header: '<pre>' },
      {
        type: 'chunk',
        html: lines
          .slice(0, 250)
          .map((line) => line.html)
          .join(''),
        offset: 250,
      },
      { type: 'chunk', html: '<span>250</span>', offset: 251 },
      { type: 'done' },
    ]);
  });

  it('splits before exceeding the 32,000-character soft HTML limit', async () => {
    const lines = [
      { html: 'a'.repeat(16_000), offset: 1 },
      { html: 'b'.repeat(16_000), offset: 2 },
      { html: 'c', offset: 3 },
    ];
    const { messages, send } = await loadWorker(async () =>
      createEngine(vi.fn(() => ({ header: '<pre>', lines }))),
    );

    await send('source', 'typescript');

    expect(messages).toEqual([
      { type: 'start', header: '<pre>' },
      { type: 'chunk', html: `${lines[0]!.html}${lines[1]!.html}`, offset: 2 },
      { type: 'chunk', html: 'c', offset: 3 },
      { type: 'done' },
    ]);
  });

  it('preserves UTF-16 offsets for Unicode CRLF source with the real engine', async () => {
    const completeLines = 'const emoji = "😀";\r\n'.repeat(250);
    const source = `${completeLines}const café = "é";`;
    const rendered = (await loadHighlighter()).render(source, 'typescript');
    const lineOffsets = rendered.lines.map((line) => line.offset);
    const { messages, send } = await loadWorker(loadHighlighter);

    await send(source, 'typescript');

    const chunkOffsets = messages.flatMap((message) => {
      if (typeof message !== 'object' || message === null) return [];
      const candidate = message as { offset?: unknown; type?: unknown };
      return candidate.type === 'chunk' && typeof candidate.offset === 'number'
        ? [candidate.offset]
        : [];
    });
    expect(lineOffsets).toContain(completeLines.length);
    expect(chunkOffsets).toContain(source.length);
    for (const offset of chunkOffsets) expect(lineOffsets).toContain(offset);
    expect(messages.at(-1)).toEqual({ type: 'done' });
  });

  it('accepts the 512 KiB boundary and declines larger sources before loading', async () => {
    const loader = vi.fn(async () =>
      createEngine(
        vi.fn((source) => ({
          header: '<pre>',
          lines: [{ html: source, offset: source.length }],
        })),
      ),
    );
    const { messages, send } = await loadWorker(loader);

    await send('x'.repeat(512 * 1024), 'typescript');
    await send('x'.repeat(512 * 1024 + 1), 'typescript');

    expect(loader).toHaveBeenCalledOnce();
    expect(messages[0]).toEqual({ type: 'start', header: '<pre>' });
    expect(messages[1]).toMatchObject({ type: 'chunk', offset: 512 * 1024 });
    expect(messages[2]).toEqual({ type: 'done' });
    expect(messages[3]).toEqual({ type: 'error' });
  });

  it.each([
    [
      'empty source',
      '',
      'typescript',
      async (): Promise<HighlightEngine> => createEngine(vi.fn()),
    ],
    [
      'NUL-containing source',
      'const value = "\0";',
      'typescript',
      loadHighlighter,
    ],
    ['unknown language', 'content', 'not-a-language', loadHighlighter],
    [
      'failed import',
      'content',
      'typescript',
      async (): Promise<HighlightEngine> =>
        Promise.reject(new Error('WASM diagnostics')),
    ],
    [
      'failed render',
      'content',
      'typescript',
      async (): Promise<HighlightEngine> =>
        createEngine(
          vi.fn(() => {
            throw new Error('tokenizer diagnostics');
          }),
        ),
    ],
  ])(
    'reports %s only as a content-free error',
    async (_case, source, language, loader): Promise<void> => {
      const diagnostics = vi
        .spyOn(console, 'error')
        .mockImplementation(() => undefined);
      const { messages, send } = await loadWorker(loader);

      await send(source, language);

      expect(messages).toEqual([{ type: 'error' }]);
      expect(diagnostics).not.toHaveBeenCalled();
    },
  );
});
