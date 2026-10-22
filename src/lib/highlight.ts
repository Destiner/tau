import { shallowRef } from 'vue';

import type { HighlightEngine } from './highlight-engine';
import resolveLanguage from './highlight-language';
import { highlightHtml } from './highlight-render';

/**
 * A grammar runs over the whole block on every delta of a streamed answer, so a
 * block long enough to be felt is left plain instead. Around five hundred
 * lines: past the point where highlighting is what the reader is waiting for.
 */
const MAX_LENGTH = 20_000;

/**
 * Re-mounting a virtualized row re-renders its markdown, and a streaming
 * message re-renders it per delta, so the same blocks are asked for over and
 * over. The bound is on entries because every prefix of a stream is its own.
 */
const MAX_CACHED = 200;

const cache = new Map<string, string>();

const highlighter = shallowRef<HighlightEngine | null>(null);

void load()
  .then((core) => {
    highlighter.value = core;
  })
  .catch(() => {
    console.error('Could not load the syntax highlighter');
  });

/** Nothing about highlighting is loaded until the app itself has been. */
async function load(): Promise<HighlightEngine> {
  const { default: loadHighlighter } = await import('./highlight-engine');
  return loadHighlighter();
}

function remember(key: string, html: string): void {
  if (cache.size >= MAX_CACHED) {
    const oldest = cache.keys().next();
    if (!oldest.done) cache.delete(oldest.value);
  }
  cache.set(key, html);
}

/**
 * Highlights a fenced block, or reports that it stays as it was written. A
 * highlighter that has not loaded yet, an unknown language, an oversized block,
 * and a grammar that cannot read what it was given all mean the same thing to
 * the caller.
 */
function highlightCode(code: string, fence: string): string | null {
  const core = highlighter.value;
  if (!core || code.length > MAX_LENGTH || code.includes('\0')) return null;

  const resolved = resolveLanguage(fence);
  if (!resolved) return null;
  const { tag } = resolved;

  const key = `${tag}\n${code}`;
  const cached = cache.get(key);
  if (cached !== undefined) return cached;

  let html: string;
  try {
    html = highlightHtml(core.render(code, tag));
  } catch {
    console.error('Could not highlight the code block');
    return null;
  }

  remember(key, html);
  return html;
}

export default highlightCode;
