const languages = {
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

const aliases = new Map(
  Object.entries(languages).flatMap(([language, labels]) =>
    [language, ...labels].map((label) => [label, language] as const),
  ),
);

export default function resolveLanguage(
  fence: string,
): { tag: string; language: string } | null {
  const tag = /^\S+/.exec(fence)?.[0].toLowerCase();
  if (!tag) return null;
  const language = aliases.get(tag);
  return language ? { tag, language } : null;
}
