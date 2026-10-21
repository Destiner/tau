import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import 'vue';

const { render, forgetTextWidths } = vi.hoisted(() => ({
  render: vi.fn(() => ({
    svg: '<svg><title id="title">Example</title></svg>',
  })),
  forgetTextWidths: vi.fn(),
}));
vi.mock('pele', () => ({ render, forgetTextWidths }));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function flush(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

async function renderer(): Promise<typeof import('./mermaid').default> {
  return (await import('./mermaid')).default;
}

describe('diagram initialization and bounded layout caching', () => {
  it('waits for bundled fonts before publishing readiness or measuring', async () => {
    let resolve!: (faces: object[]) => void;
    const pending = new Promise<object[]>((done) => {
      resolve = done;
    });
    vi.stubGlobal('document', { fonts: { load: vi.fn(() => pending) } });
    const draw = await renderer();
    expect(draw('graph TD\n A --> B', 'mermaid')).toBeNull();
    await flush();
    expect(draw('graph TD\n A --> B', 'mermaid')).toBeNull();
    expect(render).not.toHaveBeenCalled();
    resolve([{}]);
    await flush();
    expect(draw('graph TD\n A --> B', 'mermaid')).toContain('<svg');
    expect(forgetTextWidths).toHaveBeenCalledOnce();
  });

  it('recovers from failed and timed-out font initialization without caching fallback geometry', async () => {
    const load = vi
      .fn()
      .mockRejectedValueOnce(new Error('private font error'))
      .mockImplementation(() => new Promise(() => {}));
    vi.stubGlobal('document', { fonts: { load } });
    const draw = await renderer();
    expect(draw('graph TD\n A --> B', 'mermaid')).toBeNull();
    await flush();
    await vi.advanceTimersByTimeAsync(6_000);
    expect(render).not.toHaveBeenCalled();
    load.mockResolvedValue([{}]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(draw('graph TD\n A --> B', 'mermaid')).toContain('<svg');
    expect(render).toHaveBeenCalledOnce();
  });

  it('caches layout, not instance IDs, and evicts the oldest of fifty sources', async () => {
    const draw = await renderer();
    expect(draw('graph TD\n A --> B', 'ts')).toBeNull();
    expect(render).not.toHaveBeenCalled();
    draw('graph TD\n A --> B', 'mermaid');
    await flush();
    const first = draw('graph TD\n A --> B', 'mermaid');
    expect(draw('graph TD\n A --> B', 'mermaid')).not.toBe(first);
    expect(render).toHaveBeenCalledOnce();
    expect(render).toHaveBeenLastCalledWith(
      'graph TD\n A --> B',
      expect.objectContaining({
        limit: 8_000,
        outputLimit: 4_000_000,
        maxEdges: 5_000,
        links: false,
        images: false,
        responsive: false,
        autoDirection: false,
      }),
    );
    for (let i = 0; i < 50; i++) draw(`graph TD\n A${i} --> B`, 'mermaid');
    expect(render).toHaveBeenCalledTimes(51);
    draw('graph TD\n A --> B', 'mermaid');
    expect(render).toHaveBeenCalledTimes(52);
  });

  it('keeps renderer/limit failures readable and out of diagnostics', async () => {
    const draw = await renderer();
    draw('graph TD\n A --> B', 'mermaid');
    await flush();
    const error = vi.spyOn(console, 'error');
    const debug = vi.spyOn(console, 'debug');
    render.mockImplementationOnce(() => {
      throw new Error('private source limit');
    });
    expect(draw('graph TD\n A --> B', 'mermaid')).toBeNull();
    expect(draw('x'.repeat(8_001), 'mermaid')).toBeNull();
    expect(error).not.toHaveBeenCalled();
    expect(debug).not.toHaveBeenCalled();
    error.mockRestore();
    debug.mockRestore();
  });
});
