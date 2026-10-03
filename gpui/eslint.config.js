import js from '@eslint/js';
import { configs as tsConfigs } from 'typescript-eslint';
import configPrettier from 'eslint-config-prettier/flat';
import globals from 'globals';

export default [
  {
    ignores: [
      'node_modules/**',
      'target/**',
      '.cache/**',
      'review/**',
      'results/**',
    ],
  },
  js.configs.recommended,
  ...tsConfigs.recommended,
  {
    files: ['**/*.ts'],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node, Bun: 'readonly' },
    },
    rules: { '@typescript-eslint/explicit-function-return-type': 'error' },
  },
  configPrettier,
];
