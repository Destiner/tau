import { run } from '@tauri-apps/cli';

import runTauri from './tauri-command.ts';

try {
  await runTauri(process.argv.slice(2), (args) => run(args, 'bun tauri'));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
