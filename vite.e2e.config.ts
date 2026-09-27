import { defineConfig, mergeConfig } from 'vite';

import baseConfig from './vite.config';

export default defineConfig(async (env) =>
  mergeConfig(await baseConfig(env), {
    build: { outDir: 'node_modules/.cache/tau-e2e' },
    define: { 'import.meta.env.DEV': 'true' },
  }),
);
