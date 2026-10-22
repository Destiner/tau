import { describe, expect, it } from 'vitest';

import resolveLanguage from './highlight-language';

const allowlist = {
  bash: ['shellscript', 'sh', 'shell', 'zsh', 'console', 'shell-session'],
  css: [],
  diff: ['patch'],
  go: [],
  html: ['html-derivative'],
  javascript: ['js', 'cjs', 'mjs'],
  json: [],
  markdown: ['md', 'markdown-vue'],
  python: ['py'],
  rust: ['rs'],
  toml: [],
  tsx: ['jsx'],
  typescript: ['ts', 'cts', 'mts'],
  vue: [
    'vue-directives',
    'vue-interpolations',
    'vue-sfc-style-variable-injection',
  ],
  yaml: ['yml'],
} as const;

describe('highlight language fences', () => {
  it('accepts exactly the supported language and alias allowlist', () => {
    for (const [language, aliases] of Object.entries(allowlist)) {
      for (const tag of [language, ...aliases]) {
        expect(resolveLanguage(tag), tag).toEqual({ tag, language });
        expect(resolveLanguage(`${tag.toUpperCase()} options`), tag).toEqual({
          tag,
          language,
        });
      }
    }
  });

  it('uses only the first fence word and rejects leading whitespace and near misses', () => {
    expect(resolveLanguage('TS meta here')).toEqual({
      tag: 'ts',
      language: 'typescript',
    });
    expect(resolveLanguage(' TS meta here')).toBeNull();
    expect(resolveLanguage('')).toBeNull();
    expect(resolveLanguage('   \t')).toBeNull();
    expect(resolveLanguage('typescriptx')).toBeNull();
    expect(resolveLanguage('text')).toBeNull();
    expect(resolveLanguage('c++')).toBeNull();
    expect(resolveLanguage(' tsx-like')).toBeNull();
  });
});
