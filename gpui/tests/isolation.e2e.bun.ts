import { expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { verifyArchive } from '../scripts/prepare';

test('focus rendering does not inherit the preceding scenario input modality', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-focus-isolation-'));
  try {
    for (const browser of ['chromium', 'webkit'] as const) {
      const full = suite('light');
      const target = full.scenarios.find((s) => s.id === 'button-focus-blur')!;
      const keyboard = full.scenarios.find((s) => s.id === 'button-keyboard')!;
      const alone = { ...full, scenarios: [target] };
      const prefixed = { ...full, scenarios: [keyboard, target] };
      await runBrowser(alone, join(root, browser, 'alone'), browser);
      await runBrowser(prefixed, join(root, browser, 'prefixed'), browser);
      for (const step of target.steps) {
        if (step.action !== 'checkpoint') continue;
        const image = `${target.id}--${step.name}.png`;
        expect(await readFile(join(root, browser, 'alone', image))).toEqual(
          await readFile(join(root, browser, 'prefixed', image)),
        );
      }
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);

test('dependency preparation rejects a changed archive', () => {
  expect(() => verifyArchive(new Uint8Array([1, 2, 3]))).toThrow(
    'checksum mismatch',
  );
});
