import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import {
  assertCaptures,
  stateDifferences,
  type Capture,
  type Suite,
} from '../contract';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { prepare } from '../scripts/prepare';
import { exec, native } from '../scripts/native-test-helper';

const ids = new Set([
  'textarea-bare',
  'textarea-blank-lines-and-selection',
  'textarea-undo-and-shortcuts',
  'textarea-prop-retention',
  'textarea-unicode-navigation',
  'textarea-narrow-reflow',
  'textarea-long-scroll',
  'textarea-pointer-selection',
  'textarea-wrapped-rows',
  'textarea-line-boundaries',
  'textarea-edge-values',
]);
function selected(theme: 'light' | 'dark'): Suite {
  const s = suite(theme, 'textarea');
  s.scenarios = s.scenarios.filter((scenario) => ids.has(scenario.id));
  return s;
}
function verify(s: Suite, captures: Capture[]): void {
  assertCaptures(s, captures, 'textarea');
  for (const scenario of s.scenarios)
    for (const step of scenario.steps) {
      if (step.action !== 'checkpoint') continue;
      const actual = captures.find(
        (capture) =>
          capture.scenario === scenario.id && capture.checkpoint === step.name,
      )!;
      expect(
        stateDifferences(step.expected, actual.observation),
        `${scenario.id}/${step.name}`,
      ).toEqual([]);
    }
}
async function image(path: string): Promise<PNG> {
  return PNG.sync.read(await readFile(path));
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

test('authored multiline behavior matches real Chromium, WebKit and Metal in both themes; scrolling paints and returns', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-textarea-'));
  try {
    for (const theme of ['light', 'dark'] as const) {
      const s = selected(theme);
      const snapshots: Capture[][] = [];
      for (const engine of ['chromium', 'webkit', 'native'] as const) {
        const dir = join(root, theme, engine);
        const captures =
          engine === 'native'
            ? (await native(s, dir)).captures
            : await runBrowser(s, dir, engine);
        expect(captures, `${engine} capture available`).toBeDefined();
        verify(s, captures!);
        snapshots.push(captures!);
        const bare = (checkpoint: string): Capture =>
          captures!.find(
            (c) =>
              c.scenario === 'textarea-bare' && c.checkpoint === checkpoint,
          )!;
        expect(bare('fixed-height').bounds.height).toBe(120);
        expect(bare('natural-height').bounds.height).toBe(
          bare('idle').bounds.height,
        );
        expect(bare('natural-height').bounds.height).toBeLessThan(120);
        const path = (checkpoint: string): string =>
          join(dir, `textarea-long-scroll--${checkpoint}.png`);
        const top = await image(path('top'));
        const bottom = await image(path('scrolled'));
        const returned = await image(path('returned'));
        expect(
          bottom.data.equals(top.data),
          `${engine}: real scroll changes pixels`,
        ).toBe(false);
        expect(
          returned.data.equals(top.data),
          `${engine}: scroll restores pixels`,
        ).toBe(true);
        const bounds = captures!.find(
          (capture) =>
            capture.scenario === 'textarea-long-scroll' &&
            capture.checkpoint === 'top',
        )!.bounds;
        let inside = 0;
        for (let y = 0; y < top.height; y++)
          for (let x = 0; x < top.width; x++) {
            const offset = (y * top.width + x) * 4;
            if (
              top.data
                .subarray(offset, offset + 4)
                .equals(bottom.data.subarray(offset, offset + 4))
            )
              continue;
            const logicalX = x / s.scale,
              logicalY = y / s.scale;
            if (
              logicalX < bounds.x - 1 ||
              logicalX > bounds.x + bounds.width + 1 ||
              logicalY < bounds.y - 1 ||
              logicalY > bounds.y + bounds.height + 1
            )
              throw new Error(
                `${engine}: wheel painted outside the textarea at ${x}, ${y}`,
              );
            inside++;
          }
        expect(inside, `${engine}: scrolled content repaints`).toBeGreaterThan(
          100,
        );
      }
      const nativeCaptures = snapshots[2]!;
      for (const browserCaptures of snapshots.slice(0, 2))
        for (const [index, capture] of browserCaptures.entries())
          expect(
            capture.observation,
            `${theme}/${capture.scenario}/${capture.checkpoint}`,
          ).toEqual(nativeCaptures[index]!.observation);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 300000);
