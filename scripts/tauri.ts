import { run } from '@tauri-apps/cli';

import runTauri from './tauri-command.ts';

// Run with Node: Bun 1.3 can stall Vite's listen promise after a port collision.
try {
  await runTauri(process.argv.slice(2), (args) => run(args, 'bun tauri'));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
