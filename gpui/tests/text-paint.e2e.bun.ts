import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import type { Bounds, Suite } from '../contract';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { prepare } from '../scripts/prepare';
import { exec, native } from '../scripts/native-test-helper';

export function inkCoverage(
  image: PNG,
  bounds: Bounds,
  bg: number[],
  fg: number[],
): number {
  const vector = bg.map((b, i) => b - fg[i]!);
  const norm = vector.reduce((n, v) => n + v * v, 0);
  let ink = 0;
  for (let y = Math.ceil((bounds.y + 3) * 2); y < (bounds.y + 27) * 2; y++)
    for (
      let x = Math.ceil((bounds.x + 6) * 2);
      x < (bounds.x + bounds.width - 6) * 2;
      x++
    ) {
      const offset = (y * image.width + x) * 4;
      const coverage =
        vector.reduce(
          (n, v, c) => n + v * (bg[c]! - image.data[offset + c]!),
          0,
        ) / norm;
      ink += Math.max(0, Math.min(1, coverage));
    }
  return ink;
}

beforeAll(async () => {
  await prepare();
  const build = await exec('cargo', [
    'build',
    '--locked',
    '--bin',
    'tau-gpui-prototype',
  ]);
  if (build.code !== 0) throw new Error(build.output);
}, 600000);

test('real textarea glyph coverage matches WebKit without stroke thickening or a lighter colour substitution', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-text-ink-'));
  try {
    for (const theme of ['light', 'dark'] as const) {
      const s: Suite = { ...suite(theme, 'textarea'), scenarios: [] };
      for (const monospaced of [false, true]) {
        for (const placeholder of [false, true]) {
          const value = placeholder ? '' : 'The quick brown fox 0123456789';
          s.scenarios.push({
            id: `ink-${monospaced ? 'mono' : 'inter'}-${placeholder ? 'placeholder' : 'value'}`,
            component: 'textarea',
            props: {
              value,
              placeholder: placeholder ? 'Write a note' : '',
              monospaced,
            },
            steps: [
              {
                action: 'checkpoint',
                name: 'idle',
                expected: { value, changes: 0, focused: false },
              },
            ],
          });
        }
      }
      const browser = await runBrowser(
        s,
        join(root, theme, 'webkit'),
        'webkit',
      );
      const rendered = await native(s, join(root, theme, 'native'));
      expect(rendered.code, rendered.output).toBe(0);
      for (const [index, web] of browser.entries()) {
        const actual = rendered.captures![index]!;
        const load = async (engine: string): Promise<PNG> =>
          PNG.sync.read(
            await readFile(join(root, theme, engine, web.screenshot)),
          );
        const [reference, candidate] = await Promise.all([
          load('webkit'),
          load('native'),
        ]);
        const bg = theme === 'light' ? [252, 252, 252] : [16, 20, 28];
        const fg = web.scenario.endsWith('placeholder')
          ? theme === 'light'
            ? [161, 168, 178]
            : [90, 99, 120]
          : theme === 'light'
            ? [92, 97, 102]
            : [191, 189, 182];
        const expectedInk = inkCoverage(reference, web.bounds, bg, fg);
        const actualInk = inkCoverage(candidate, actual.bounds, bg, fg);
        expect(expectedInk).toBeGreaterThan(100);
        const ratio = actualInk / expectedInk;
        console.log(
          `${theme}/${web.scenario}: native/WebKit ink=${ratio.toFixed(4)}`,
        );
        // This catches the 17–30% excess before the fix, while allowing subpixel raster differences.
        expect(ratio).toBeGreaterThan(0.95);
        expect(ratio).toBeLessThan(1.05);
        let solid = 0;
        for (
          let y = Math.ceil((actual.bounds.y + 3) * 2);
          y < (actual.bounds.y + 27) * 2;
          y++
        )
          for (
            let x = Math.ceil((actual.bounds.x + 6) * 2);
            x < (actual.bounds.x + actual.bounds.width - 6) * 2;
            x++
          ) {
            const offset = (y * candidate.width + x) * 4;
            if (
              fg.every(
                (channel, c) =>
                  Math.abs(candidate.data[offset + c]! - channel) <= 1,
              )
            )
              solid++;
          }
        expect(
          solid,
          'opaque glyph ink retains the authored foreground colour',
        ).toBeGreaterThan(4);
      }
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 180000);
