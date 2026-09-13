import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],

  // Keep Rust errors visible alongside Vite output.
  clearScreen: false,
  server: {
    // The launcher passes Vite's actual URL to Tauri if this port is occupied.
    port: 1420,
    strictPort: false,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
        }
      : undefined,
    watch: {
      // Rust changes are handled by Tauri's watcher.
      ignored: ['**/src-tauri/**'],
    },
  },
}));
