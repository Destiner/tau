import { expect, test } from 'bun:test';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { PNG } from 'pngjs';
import {
  assertImagePaint,
  assertLongPaint,
  assertMixedPaint,
} from './probe-paint';

const area = { x: 20, y: 125.5, width: 860, height: 526.5 };
const imageBounds = { x: 130, y: 221.5, width: 640, height: 360 };
const frame = (): PNG => {
  const png = new PNG({ width: 1800, height: 1400 });
  png.data.fill(240);
  return png;
};
const copy = (png: PNG): PNG => PNG.sync.read(PNG.sync.write(png));
function paint(
  png: PNG,
  x: number,
  y: number,
  width: number,
  height: number,
  color: number[],
): void {
  for (let row = y; row < y + height; row++)
    for (let col = x; col < x + width; col++) {
      const i = (row * png.width + col) * 4;
      png.data.set([...color, 255], i);
    }
}

test('mixed fences require colored JSON glyphs and neutral visible unknown glyphs', () => {
  const png = frame();
  paint(png, 120, 600, 25, 18, [20, 80, 190]);
  paint(png, 170, 600, 25, 18, [20, 80, 190]);
  paint(png, 120, 755, 40, 18, [70, 70, 70]);
  assertMixedPaint(png, area);
  const missing = copy(png);
  paint(missing, 120, 600, 75, 18, [240, 240, 240]);
  expect(() => assertMixedPaint(missing, area)).toThrow('JSON fence');
  const shifted = copy(png);
  paint(shifted, 120, 600, 75, 18, [240, 240, 240]);
  paint(shifted, 120, 670, 75, 18, [20, 80, 190]);
  expect(() => assertMixedPaint(shifted, area)).toThrow('JSON fence');
  const coloredUnknown = copy(png);
  paint(coloredUnknown, 120, 755, 40, 18, [20, 80, 190]);
  expect(() => assertMixedPaint(coloredUnknown, area)).toThrow('unknown fence');
});

test('horizontal wheel changes only the bounded code row, not the pinned action', () => {
  const before = frame();
  const after = copy(before);
  paint(after, 120, 480, 80, 20, [20, 80, 190]);
  assertLongPaint(before, after, area);
  expect(() => assertLongPaint(before, copy(before), area)).toThrow('code row');
  const actionMoved = copy(after);
  paint(actionMoved, 1550, 480, 30, 20, [20, 80, 190]);
  expect(() => assertLongPaint(before, actionMoved, area)).toThrow(
    'copy action',
  );
});

test('image bounds alone cannot pass if native image pixels are missing or displaced', async () => {
  const fixture = PNG.sync.read(
    await readFile(
      resolve(import.meta.dir, '../assets/probes/files/landscape.png'),
    ),
  );
  const png = frame();
  const samples = [
    [40, 30],
    [160, 30],
    [270, 30],
    [40, 90],
    [160, 90],
    [270, 90],
    [40, 150],
    [160, 150],
    [270, 150],
  ];
  for (const [x, y] of samples) {
    const i = (y! * fixture.width + x!) * 4;
    const color = [...fixture.data.subarray(i, i + 3)];
    paint(
      png,
      Math.round((imageBounds.x + (x! + 0.5) * 2) * 2),
      Math.round((imageBounds.y + (y! + 0.5) * 2) * 2),
      1,
      1,
      color,
    );
  }
  assertImagePaint(png, fixture, imageBounds);
  const hidden = copy(png);
  paint(hidden, 0, 400, 1800, 800, [240, 240, 240]);
  expect(() => assertImagePaint(hidden, fixture, imageBounds)).toThrow(
    'native image pixel mismatch',
  );
  const shifted = copy(png);
  paint(shifted, 0, 400, 1800, 800, [240, 240, 240]);
  for (const [x, y] of samples) {
    const i = (y! * fixture.width + x!) * 4;
    paint(
      shifted,
      Math.round((imageBounds.x + (x! + 0.5) * 2) * 2) + 20,
      Math.round((imageBounds.y + (y! + 0.5) * 2) * 2),
      1,
      1,
      [...fixture.data.subarray(i, i + 3)],
    );
  }
  expect(() => assertImagePaint(shifted, fixture, imageBounds)).toThrow(
    'native image pixel mismatch',
  );
});
