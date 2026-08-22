import bash from '@shikijs/langs/bash';
import css from '@shikijs/langs/css';
import diff from '@shikijs/langs/diff';
import go from '@shikijs/langs/go';
import html from '@shikijs/langs/html';
import javascript from '@shikijs/langs/javascript';
import json from '@shikijs/langs/json';
import markdown from '@shikijs/langs/markdown';
import python from '@shikijs/langs/python';
import rust from '@shikijs/langs/rust';
import toml from '@shikijs/langs/toml';
import tsx from '@shikijs/langs/tsx';
import typescript from '@shikijs/langs/typescript';
import vue from '@shikijs/langs/vue';
import yaml from '@shikijs/langs/yaml';
import ayuDark from '@shikijs/themes/ayu-dark';
import ayuLight from '@shikijs/themes/ayu-light';
import { createHighlighterCore, type HighlighterCore } from 'shiki/core';
import { createOnigurumaEngine } from 'shiki/engine/oniguruma';

/**
 * Builds the highlighter: a megabyte of grammars, both of Ayu's schemes, and
 * the engine that reads them. It is all in this module so that it is all one
 * chunk, loaded after the window is and parsed before no transcript.
 *
 * Oniguruma rather than Shiki's JavaScript regex engine, which needs no
 * WebAssembly but does not agree with JavaScriptCore: in WebKit it loses the
 * comment scope of every grammar tried, and at its default target it collapses
 * most of a line into a single token. WKWebView is the app's own runtime, so
 * the engine that is right there is the one worth waiting for.
 *
 * The languages are the ones worth their grammar — what this project's agent
 * output is written in, plus the configuration and shell it talks about.
 * Anything else stays plain rather than pulling another grammar in.
 */
function loadHighlighter(): Promise<HighlighterCore> {
  return createHighlighterCore({
    themes: [ayuLight, ayuDark],
    langs: [
      bash,
      css,
      diff,
      go,
      html,
      javascript,
      json,
      markdown,
      python,
      rust,
      toml,
      tsx,
      typescript,
      vue,
      yaml,
    ],
    engine: createOnigurumaEngine(import('shiki/wasm')),
  });
}

export default loadHighlighter;
