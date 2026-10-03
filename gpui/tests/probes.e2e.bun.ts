import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from '@playwright/test';
import { PNG } from 'pngjs';
import { exec } from '../scripts/native-test-helper';
import { prepare } from '../scripts/prepare';
import {
  captureTranscript,
  transcriptReport,
} from '../scripts/transcript-report';

const binaries = resolve(import.meta.dir, '../target/debug');
beforeAll(async () => {
  await prepare();
  const build = await exec('cargo', ['build', '--locked', '--bins']);
  if (build.code) throw new Error(build.output);
}, 600000);

test('live multiline host routes clicks/Space and preserves the draft, selection and undo across controls', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'tau-playground-test-'));
  try {
    const run = await exec(join(binaries, 'tau-gpui-prototype'), [
      '--playground-capture',
      dir,
    ]);
    expect(run.code, run.output).toBe(0);
    const checks = JSON.parse(
      await readFile(join(dir, 'checks.json'), 'utf8'),
    ) as {
      checkpoint: string;
      screenshot: string;
      observation: { value: string };
    }[];
    expect(checks.map((c) => c.checkpoint)).toEqual([
      'initial',
      'edited',
      'width-narrow',
      'height-short',
      'dark',
      'light-keyboard',
      'dark-again',
      'mono',
      'bare',
      'readonly',
      'readonly-off',
      'disabled',
      'disabled-off',
      'width-wide',
      'height-tall',
      'clear',
      'sample',
      'long',
    ]);
    for (const c of checks) {
      const png = PNG.sync.read(await readFile(join(dir, c.screenshot)));
      expect([png.width, png.height]).toEqual([1600, 1300]);
    }
    expect(
      checks.find((c) => c.checkpoint === 'clear')!.observation.value,
    ).toBe('');
    expect(
      checks.find((c) => c.checkpoint === 'long')!.observation.value.length,
    ).toBeGreaterThan(4000);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 120000);

test('native transcript capture, independent anchor fault, report controls and artifact freshness', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'tau-transcript-test-'));
  try {
    await captureTranscript(dir);
    await transcriptReport(dir);
    const browser = await chromium.launch();
    try {
      const page = await browser.newPage();
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.goto(pathToFileURL(join(dir, 'report.html')).href);
      expect(await page.locator('article:visible').count()).toBe(6);
      expect(
        await page
          .locator('img')
          .evaluateAll((images) =>
            images.every(
              (image) =>
                (image as HTMLImageElement).complete &&
                (image as HTMLImageElement).naturalWidth === 1520,
            ),
          ),
      ).toBe(true);
      await page.locator('#theme').selectOption('dark');
      expect(await page.locator('section[data-theme="dark"]').isVisible()).toBe(
        true,
      );
      expect(
        await page.locator('section[data-theme="light"]').isVisible(),
      ).toBe(false);
      expect(errors).toEqual([]);
    } finally {
      await browser.close();
    }
    const fault = await exec(join(binaries, 'transcript-probe'), [
      '--capture',
      join(dir, 'fault'),
      '--fault-anchor',
    ]);
    expect(fault.code).not.toBe(0);
    const broken = JSON.parse(
      await readFile(join(dir, 'fault/checks.json'), 'utf8'),
    ) as { name: string; passed: boolean }[];
    expect(broken.some((c) => c.name === 'reader' && !c.passed)).toBe(true);
    const file = join(dir, 'light/checks.json');
    const valid = await readFile(file, 'utf8');
    await writeFile(file, valid.replace('following', 'tampered'));
    await expect(transcriptReport(dir)).rejects.toThrow('Changed');
    await writeFile(file, valid);
    const proofFile = join(dir, 'light/capture.json');
    const proof = JSON.parse(await readFile(proofFile, 'utf8'));
    await writeFile(proofFile, JSON.stringify({ ...proof, binary: 'stale' }));
    await expect(transcriptReport(dir)).rejects.toThrow('stale');
    const rejected = await exec(join(binaries, 'transcript-probe'), [
      '--fault-anchor',
    ]);
    expect(rejected.code).not.toBe(0);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}, 300000);
