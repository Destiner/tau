import { expect, test } from 'bun:test';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
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

test('parallel report writes cannot hot-reload a browser capture', async () => {
  const artifacts = resolve(import.meta.dir, '../results');
  await mkdir(artifacts, { recursive: true });
  const root = await mkdtemp(join(artifacts, 'watcher-test-'));
  try {
    const s = suite('light', 'textarea');
    s.scenarios = s.scenarios.filter((s) => s.id === 'textarea-empty');
    await runBrowser(s, join(root, 'captures'), 'chromium', async (page) => {
      await page.evaluate(() => {
        document.body.dataset.captureSentinel = 'retained';
      });
      await writeFile(
        join(root, 'unrelated-report.html'),
        '<h1>Parallel report</h1>',
      );
      // Give a mistakenly enabled filesystem watcher time to dispatch its reload.
      await page.waitForTimeout(600);
      expect(
        await page.evaluate(() => document.body.dataset.captureSentinel),
      ).toBe('retained');
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 30000);

test('dependency preparation rejects a changed archive', () => {
  expect(() => verifyArchive(new Uint8Array([1, 2, 3]))).toThrow(
    'checksum mismatch',
  );
});
