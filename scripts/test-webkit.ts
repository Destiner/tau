import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);

if (args[0] === '--probe') {
  const { webkit } = await import('@playwright/test');
  const browser = await webkit.launch({ timeout: 10_000 });
  try {
    const page = await browser.newPage();
    await page.setContent(
      '<button onclick="this.textContent=\'ready\'">probe</button>',
    );
    await page.locator('button').click({ timeout: 2_000 });
    if ((await page.locator('button').textContent()) !== 'ready') {
      throw new Error('WebKit interaction failed');
    }
  } finally {
    await browser.close();
  }
} else {
  const mode = args.shift();
  if (mode !== 'webkit' && mode !== 'ci') {
    throw new Error('Expected WebKit mode: webkit or ci');
  }

  type RunResult = { code: number; signal?: NodeJS.Signals };

  // Own a process group so a stalled browser cannot outlive the bounded probe.
  const run = (
    command: string,
    argv: string[],
    probe = false,
  ): Promise<RunResult> =>
    new Promise((resolve) => {
      const child = spawn(command, argv, {
        detached: true,
        stdio: probe ? 'ignore' : 'inherit',
        env: { ...process.env, TAU_PLAYWRIGHT_MODE: mode },
      });
      let stopped = false;
      let interrupted: NodeJS.Signals | undefined;
      const killGroup = (signal: NodeJS.Signals): void => {
        if (child.pid) {
          try {
            process.kill(-child.pid, signal);
          } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== 'ESRCH') throw error;
          }
        }
      };
      let escalation: ReturnType<typeof setTimeout> | undefined;
      const stop = (signal: NodeJS.Signals): void => {
        if (stopped) return;
        stopped = true;
        killGroup(signal);
        escalation = setTimeout(() => killGroup('SIGKILL'), 500);
      };
      const onInterrupt = (signal: NodeJS.Signals): void => {
        interrupted = signal;
        stop(signal);
      };
      process.on('SIGINT', onInterrupt);
      process.on('SIGTERM', onInterrupt);
      const deadline = probe
        ? setTimeout(() => stop('SIGTERM'), 14_000)
        : undefined;
      let finished = false;
      const finish = (
        code: number | null,
        signal?: NodeJS.Signals | null,
      ): void => {
        if (finished) return;
        finished = true;
        clearTimeout(deadline);
        // Always reap owned descendants, even if the launcher exited first.
        killGroup('SIGKILL');
        clearTimeout(escalation);
        process.off('SIGINT', onInterrupt);
        process.off('SIGTERM', onInterrupt);
        resolve({
          code: stopped || signal ? 1 : (code ?? 1),
          signal: interrupted ?? (stopped ? undefined : (signal ?? undefined)),
        });
      };
      child.once('error', () => finish(1));
      child.once('exit', finish);
    });

  let result: RunResult = { code: 0 };
  if (!args.includes('--list')) {
    result = await run(
      process.execPath,
      [fileURLToPath(import.meta.url), '--probe'],
      true,
    );
    if (result.code !== 0 && !result.signal) {
      console.error(
        'WebKit startup probe failed (15s budget), before starting Tau. Install the locked browser with `bun x playwright install webkit` on a supported desktop/CI host. In the background agent VM, use `bun run test:e2e` and leave WebKit to CI.',
      );
    }
  }
  if (result.code === 0 && !result.signal) {
    result = await run(process.execPath, ['x', 'playwright', 'test', ...args]);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  } else {
    process.exitCode = result.code;
  }
}
