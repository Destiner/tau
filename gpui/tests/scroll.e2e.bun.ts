import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import { assertCaptures, stateDifferences, type Capture } from '../contract';
import { prepare } from '../scripts/prepare';
import { exec, native } from '../scripts/native-test-helper';
import { scrollFixture } from './scroll-fixture';

function color(image: PNG, x: number, y: number): number[] {
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
// Measure movement of the real text, without transforming stored frames or parity metrics.
function displacement(before: PNG, after: PNG): number {
  const errors = Array.from({ length: 65 }, (_, shift) => {
    let error = 0;
    for (let y = 86; y < 198; y++)
      for (let x = 70; x < 450; x++)
        error += distance(color(before, x, y + shift), color(after, x, y));
    return error;
  });
  return errors.indexOf(Math.min(...errors));
}
function thumb(
  image: PNG,
  bounds: Capture['bounds'],
): { x: number; y: number; width: number; height: number } {
  const background = color(
    image,
    (bounds.x + bounds.width - 20) * 2,
    (bounds.y + 60) * 2,
  );
  let minX = image.width,
    maxX = -1,
    minY = image.height,
    maxY = -1;
  // Exclude the input frame's rounded corners, not the thumb's end caps.
  for (let y = (bounds.y + 4) * 2; y < (bounds.y + bounds.height - 4) * 2; y++)
    for (
      let x = (bounds.x + bounds.width - 15) * 2;
      x < (bounds.x + bounds.width - 2) * 2;
      x++
    ) {
      if (distance(color(image, x, y), background) <= 3) continue;
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
  expect(maxX - minX + 1, 'visible thumb width').toBeGreaterThanOrEqual(10);
  expect(maxY - minY + 1, 'visible thumb height').toBeGreaterThan(60);
  const cx = Math.floor((minX + maxX) / 2),
    cy = Math.floor((minY + maxY) / 2);
  expect(
    distance(color(image, cx, cy), background),
    'solid thumb body',
  ).toBeGreaterThan(3);
  for (const [x, y] of [
    [minX, minY],
    [maxX, minY],
    [minX, maxY],
    [maxX, maxY],
  ])
    expect(
      distance(color(image, x!, y!), background),
      'rounded corner leaves canvas visible',
    ).toBeLessThanOrEqual(3);
  expect(
    distance(color(image, cx, minY), background),
    'rounded top cap remains visible',
  ).toBeGreaterThan(3);
  expect(
    distance(color(image, cx, maxY), background),
    'rounded bottom cap remains visible',
  ).toBeGreaterThan(3);
  return { x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1 };
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

test('real wheel dispatch slows multiline motion, accumulates fractions and paints rounded draggable thumbs', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-scroll-'));
  try {
    for (const theme of ['light', 'dark'] as const) {
      const s = scrollFixture(theme),
        dir = join(root, theme);
      const result = await native(s, dir);
      expect(result.code, result.output).toBe(0);
      assertCaptures(s, result.captures!, 'scroll');
      for (const step of s.scenarios[0]!.steps)
        if (step.action === 'checkpoint') {
          const capture = result.captures!.find(
            (c) => c.checkpoint === step.name,
          )!;
          expect(
            stateDifferences(step.expected, capture.observation),
            `${theme}/${step.name}: draft and selection`,
          ).toEqual([]);
        }
      const frame = async (name: string): Promise<PNG> =>
        PNG.sync.read(
          await readFile(join(dir, `textarea-scroll-precision--${name}.png`)),
        );
      const top = await frame('top'),
        down = await frame('down');
      expect(
        displacement(top, down),
        '32px wheel input moves text 16 logical px at scale 2',
      ).toBe(32);
      expect(displacement(top, await frame('returned'))).toBe(0);
      expect(
        displacement(top, await frame('fractional')),
        'eight half-pixel events accumulate to 2 logical px',
      ).toBe(4);
      expect(displacement(top, await frame('clamped-top'))).toBe(0);
      const bounds = result.captures![0]!.bounds;
      const normal = thumb(down, bounds),
        hover = thumb(await frame('hover'), bounds);
      const pressed = thumb(await frame('dragging'), bounds),
        dragged = thumb(await frame('dragged'), bounds);
      expect(normal.width).toBe(12);
      expect(hover.width).toBe(16);
      expect(pressed.width).toBe(hover.width);
      expect(
        Math.abs(dragged.y - pressed.y - 40),
        '20px thumb drag is not slowed by wheel sensitivity',
      ).toBeLessThanOrEqual(1);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);
