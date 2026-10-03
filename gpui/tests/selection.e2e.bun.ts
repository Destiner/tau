import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import type { Suite, Theme } from '../contract';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { prepare } from '../scripts/prepare';
import { exec, native } from '../scripts/native-test-helper';

function selected(theme: Theme, monospaced: boolean): Suite {
  const s = structuredClone(suite(theme, 'textarea'));
  s.scenarios = s.scenarios.filter(
    (item) => item.id === 'textarea-blank-lines-and-selection',
  );
  s.scenarios[0]!.props.monospaced = monospaced;
  s.scenarios.push({
    id: 'textarea-forward-newline',
    component: 'textarea',
    props: { value: 'ab\n\ncd', monospaced },
    steps: [
      { action: 'focus' },
      { action: 'select', start: 3, end: 3 },
      { action: 'key', key: 'Shift+ArrowDown' },
      { action: 'key', key: 'Shift+ArrowRight' },
      {
        action: 'checkpoint',
        name: 'forward-row',
        expected: { value: 'ab\n\ncd', selection: [3, 5], changes: 0 },
      },
    ],
  });
  return s;
}
function pixel(image: PNG, x: number, y: number): number[] {
  return [
    ...image.data.subarray(
      (y * image.width + x) * 4,
      (y * image.width + x) * 4 + 3,
    ),
  ];
}
function distance(a: number[], b: number[]): number {
  return Math.max(...a.map((channel, i) => Math.abs(channel - b[i]!)));
}

beforeAll(async () => {
  await prepare();
  const build = await exec('cargo', [
    'build',
    '--locked',
    '--bin',
    'tau-gpui-prototype',
  ]);
  if (build.code !== 0) throw new Error(`native build failed: ${build.output}`);
}, 600000);

test('forward/backward newline selections fill and align with WebKit rows in both fonts and themes', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-selection-'));
  try {
    for (const theme of ['light', 'dark'] as const)
      for (const mono of [false, true]) {
        const s = selected(theme, mono);
        const dir = join(root, `${theme}-${mono}`);
        const webCaptures = await runBrowser(s, join(dir, 'webkit'), 'webkit');
        const result = await native(s, join(dir, 'native'));
        expect(result.code, result.output).toBe(0);
        for (const checkpoint of ['backward-row', 'forward-row']) {
          const web = webCaptures.find((c) => c.checkpoint === checkpoint)!;
          const metal = result.captures!.find(
            (c) => c.checkpoint === checkpoint,
          )!;
          expect(metal.observation).toEqual(web.observation);
          expect(metal.observation.selection).toEqual([3, 5]);
          const load = async (engine: string): Promise<PNG> =>
            PNG.sync.read(await readFile(join(dir, engine, web.screenshot)));
          const [browser, actual] = await Promise.all([
            load('webkit'),
            load('native'),
          ]);
          const left = Math.round((web.bounds.x + 9) * s.scale);
          const right = Math.round(
            (web.bounds.x + web.bounds.width - 9) * s.scale,
          );
          const top = Math.round((web.bounds.y + 18) * s.scale);
          const center = Math.round((left + right) / 2);
          const band = (image: PNG): number[] => {
            const background = pixel(image, center, top + 100);
            return Array.from(
              { length: 50 },
              (_, offset) => top + offset,
            ).filter((y) => distance(pixel(image, center, y), background) > 12);
          };
          const expectedBand = band(browser),
            actualBand = band(actual);
          expect(expectedBand.length).toBeGreaterThan(25);
          expect(actualBand.length).toBeGreaterThan(25);
          // Test the painted top/bottom, not just a pixel in the shared interior.
          expect(
            Math.abs(actualBand[0]! - expectedBand[0]!),
          ).toBeLessThanOrEqual(1);
          expect(
            Math.abs(actualBand.at(-1)! - expectedBand.at(-1)!),
          ).toBeLessThanOrEqual(1);
          const y = expectedBand[Math.floor(expectedBand.length / 2)]!;
          for (const image of [browser, actual]) {
            const background = pixel(image, center, top + 100);
            for (const x of [left + 20, center, right - 20])
              expect(
                distance(pixel(image, x, y), background),
                'selected newline fills the content row',
              ).toBeGreaterThan(12);
            for (const x of [left + 45, center])
              expect(
                distance(pixel(image, x, y + 32), background),
                'last partial row does not fill past c',
              ).toBeLessThan(12);
            expect(
              distance(pixel(image, left - 10, y), background),
              'selection does not spill into the left padding',
            ).toBeLessThan(12);
          }
        }
      }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 300000);
