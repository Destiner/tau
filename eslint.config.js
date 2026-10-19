import js from '@eslint/js';
import { configs as tsConfigs } from 'typescript-eslint';
import configPrettier from 'eslint-config-prettier/flat';
import globals from 'globals';

export default [
  {
    ignores: [
      '**/node_modules/**',
      '**/target/**',
      '**/.cache/**',
      '.worktrees/**',
      'gpui/results/**',
      'gpui/review/**',
      'dist/**',
      'test-results/**',
      'playwright-report/**',
      'src-tauri/**',
    ],
  },
  js.configs.recommended,
  ...tsConfigs.recommended,
  {
    files: ['**/*.{js,ts}'],
    languageOptions: { globals: { ...globals.node, Bun: 'readonly' } },
  },
  {
    files: ['**/*.ts'],
    rules: { '@typescript-eslint/explicit-function-return-type': 'error' },
  },
  configPrettier,
];
