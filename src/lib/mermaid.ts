import { shallowRef } from 'vue';

import { scopeDiagramSvg } from './diagram-svg';

type DiagramRenderer = typeof import('pele').render;

const OPTIONS = {
  fontFamily: 'Inter Variable',
  fontFamilyMono: 'ui-monospace, SFMono-Regular, Menlo, monospace',
  fontSize: 14,
  padding: 12,
  responsive: false,
  autoDirection: false,
  links: false,
  images: false,
  limit: 8_000,
  outputLimit: 4_000_000,
  maxEdges: 5_000,
} as const;

const MAX_CACHED = 50;
const cache = new Map<string, string>();
const renderer = shallowRef<DiagramRenderer | null>(null);
let loading = false;
let retries = 0;

async function bounded<T>(promise: Promise<T>): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error('Diagram initialization timed out')),
          5_000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function fontsReady(): Promise<void> {
  // Node tests use Pele's metrics measurer; browser layout must never cache
  // widths from a transient fallback face.
  if (typeof document === 'undefined' || !document.fonts) return;
  const fonts = await Promise.all([
    document.fonts.load('14px "Inter Variable"'),
    document.fonts.load('600 14px "Inter Variable"'),
    document.fonts.load('italic 14px "Inter Variable"'),
  ]);
  if (fonts.some((faces) => faces.length === 0))
    throw new Error('Diagram fonts unavailable');
}

/** Load only when a closed diagram fence is encountered. */
function load(): void {
  if (loading) return;
  loading = true;
  void bounded(Promise.all([import('pele'), fontsReady()]))
    .then(([module]) => {
      module.forgetTextWidths();
      renderer.value = module.render;
      retries = 0;
    })
    .catch(() => {
      // Initialization is cosmetic: readable source remains in place. Retry
      // transient font/module failures, without logging source-bearing errors.
      if (retries < 3) {
        retries += 1;
        setTimeout(load, 1_000);
      }
    })
    .finally(() => {
      loading = false;
    });
}

/** Unavailable, invalid or oversized diagrams remain readable source code. */
function renderDiagram(source: string, fence: string): string | null {
  if (!/^mermaid(?:\s|$)/i.test(fence) || source.length > OPTIONS.limit)
    return null;
  const render = renderer.value;
  if (!render) {
    load();
    return null;
  }
  const cached = cache.get(source);
  if (cached !== undefined) return scopeDiagramSvg(cached);

  try {
    const { svg } = render(source, OPTIONS);
    if (cache.size >= MAX_CACHED) {
      const oldest = cache.keys().next();
      if (!oldest.done) cache.delete(oldest.value);
    }
    cache.set(source, svg);
    return scopeDiagramSvg(svg);
  } catch {
    // Parser diagnostics can contain the user's source; never log them.
    return null;
  }
}

export default renderDiagram;
