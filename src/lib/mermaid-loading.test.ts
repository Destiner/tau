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
