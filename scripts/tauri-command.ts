import { createServer } from 'vite';

type InvokeTauri = (args: string[]) => Promise<void>;

async function runTauri(args: string[], invoke: InvokeTauri): Promise<void> {
  const separator = args.indexOf('--');
  const cliArgs = separator === -1 ? args : args.slice(0, separator);
  if (
    args[0] !== 'dev' ||
    cliArgs.some((arg) => ['--help', '-h', '--version', '-V'].includes(arg))
  ) {
    await invoke(args);
    return;
  }

  // Keep Vite listening while Tauri runs: probing and releasing a port races
  // with other worktrees starting at the same time.
  const server = await createServer();
  try {
    await server.listen();
    const devUrl =
      server.resolvedUrls?.local[0] ?? server.resolvedUrls?.network[0];
    if (!devUrl) {
      throw new Error('Vite did not expose a development server URL.');
    }
    server.printUrls();
    const override = JSON.stringify({
      build: { devUrl, beforeDevCommand: null },
    });
    const insertAt = separator === -1 ? args.length : separator;
    await invoke([
      ...args.slice(0, insertAt),
      '--config',
      override,
      ...args.slice(insertAt),
    ]);
  } finally {
    await server.close();
  }
}

export default runTauri;
