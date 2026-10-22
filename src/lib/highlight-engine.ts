import { codeToTokens, isSupportedLanguage } from '@pierre/highlights';

import resolveLanguage from './highlight-language';
import { renderHighlight } from './highlight-render';
import matchedThemes from './highlight-theme';

function render(
  source: string,
  fence: string,
): ReturnType<typeof renderHighlight> {
  // HTML parsing replaces NULs, even as entities; keep such source plain.
  if (source.includes('\0')) throw new Error('Non-HTML source');
  const resolved = resolveLanguage(fence);
  if (!resolved || !isSupportedLanguage(resolved.language))
    throw new Error('Unsupported highlight language');
  return renderHighlight(
    source,
    codeToTokens(source, {
      lang: resolved.language,
      themes: matchedThemes,
      defaultColor: false,
      cssVariablePrefix: '--tau-code-',
      // A minified line must not become an unbounded, indivisible DOM batch.
      tokenizeMaxLineLength: 20_000,
    }),
    resolved.tag,
  );
}

export interface HighlightEngine {
  render: typeof render;
}

// Importing the standalone entry initializes its embedded WASM once per realm.
export default async function loadHighlighter(): Promise<HighlightEngine> {
  return { render };
}
