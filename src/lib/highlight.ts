import type { HighlighterCore } from 'shiki/core';
import { shallowRef } from 'vue';

/**
 * Ayu's two schemes, as Shiki names the ones the engine module loads. Neither
 * scheme's own editor background is used: a code block sits on Tau's sunk
 * surface, wherever that surface happens to be.
 */
const THEMES = { light: 'ayu-light', dark: 'ayu-dark' } as const;

/*
 * Every token carries both schemes as custom properties and the stylesheet
 * chooses between them, so changing appearance re-paints a transcript instead
 * of re-rendering one.
 */
const VARIABLE_PREFIX = '--tau-code-';

/**
 * Fence tags that name a language we have, under a name Shiki does not know it
 * by. A grammar's own aliases are registered with it, so these are only the
 * ones Shiki has no opinion about.
 */
const ALIASES = new Map([
  ['console', 'bash'],
  ['jsx', 'tsx'],
  ['patch', 'diff'],
  ['shell-session', 'bash'],
]);

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

const LANGUAGE_TAG = /^\S+/;

interface FenceLanguage {
  /** The language as the fence named it, which is what a reader is shown. */
  tag: string;
  /** The grammar that tag resolves to, which is what Shiki is asked for. */
  language: string;
}

const cache = new Map<string, string>();

/*
 * Loading the grammars and the engine is asynchronous and rendering markdown is
 * not, so the highlighter is a ref: blocks rendered before it arrives are
 * plain, and reading it while rendering is what re-renders them once it has.
 */
const highlighter = shallowRef<HighlighterCore | null>(null);

void load()
  .then((core) => {
    highlighter.value = core;
  })
  .catch((error: unknown) => {
    console.error('Could not load the syntax highlighter', error);
  });

/** Nothing about highlighting is loaded until the app itself has been. */
async function load(): Promise<HighlighterCore> {
  const { default: loadHighlighter } = await import('./highlight-engine');
  return loadHighlighter();
}

/** The grammar a fence's info string asks for, if we have it. */
function resolveLanguage(
  core: HighlighterCore,
  fence: string,
): FenceLanguage | null {
  const tag = LANGUAGE_TAG.exec(fence)?.[0].toLowerCase();
  if (!tag) return null;

  const language = ALIASES.get(tag) ?? tag;
  if (!core.getLoadedLanguages().includes(language)) return null;
  return { tag, language };
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
  if (!core || code.length > MAX_LENGTH) return null;

  const resolved = resolveLanguage(core, fence);
  if (!resolved) return null;
  const { tag, language } = resolved;

  const key = `${tag}\n${code}`;
  const cached = cache.get(key);
  if (cached !== undefined) return cached;

  let html: string;
  try {
    html = core.codeToHtml(code, {
      lang: language,
      themes: THEMES,
      defaultColor: false,
      cssVariablePrefix: VARIABLE_PREFIX,
      transformers: [
        {
          // A block is content, not a control: the transcript's tab order is
          // the app's, and Shiki's own focusable `pre` is not part of it.
          pre(node): void {
            delete node.properties.tabindex;
          },
          // The class marked would have written, naming the language as the
          // fence did: it is what the block's wrapper reads its label from.
          code(node): void {
            node.properties.class = `language-${tag}`;
          },
        },
      ],
    });
  } catch (error) {
    console.error('Could not highlight the code block', error);
    return null;
  }

  remember(key, html);
  return html;
}

export default highlightCode;
