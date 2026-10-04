import { readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { PNG } from 'pngjs';

type Rect = { x: number; y: number; width: number; height: number };
type Frame = PNG;
type Check = {
  name: string;
  passed: boolean;
  screenshot: string;
  observations: Record<string, unknown>;
};

function requirePaint(ok: boolean, message: string): asserts ok {
  if (!ok) throw new Error(`probe paint: ${message}`);
}

function bounds(value: unknown): Rect {
  requirePaint(
    value !== null && typeof value === 'object',
    'missing observed bounds',
  );
  const rect = value as Rect;
  requirePaint(
    [rect.x, rect.y, rect.width, rect.height].every(Number.isFinite),
    'invalid observed bounds',
  );
  requirePaint(rect.width > 0 && rect.height > 0, 'empty observed bounds');
  return rect;
}

function roi(frame: Frame, rect: Rect): Rect {
  const r = {
    x: Math.round(rect.x),
    y: Math.round(rect.y),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
  requirePaint(
    r.x >= 0 &&
      r.y >= 0 &&
      r.width > 0 &&
      r.height > 0 &&
      r.x + r.width <= frame.width &&
      r.y + r.height <= frame.height,
    `ROI outside ${frame.width}x${frame.height}`,
  );
  return r;
}

function pixel(frame: Frame, x: number, y: number): number[] {
  const i = (y * frame.width + x) * 4;
  return [frame.data[i]!, frame.data[i + 1]!, frame.data[i + 2]!];
}

function count(
  frame: Frame,
  rect: Rect,
  predicate: (rgb: number[]) => boolean,
): number {
  const r = roi(frame, rect);
  let n = 0;
  for (let y = r.y; y < r.y + r.height; y++)
    for (let x = r.x; x < r.x + r.width; x++)
      if (predicate(pixel(frame, x, y))) n++;
  return n;
}

function chromatic(rgb: number[]): boolean {
  return Math.max(...rgb) - Math.min(...rgb) > 35;
}

// Coordinates are physical pixels relative to the observed Markdown area, at the fixture's 2x scale.
export function assertMixedPaint(frame: Frame, observed: unknown): void {
  const b = bounds(observed);
  const x = b.x * 2 + 60;
  const json = { x, y: b.y * 2 + 340, width: 440, height: 45 };
  const unknown = { x, y: b.y * 2 + 490, width: 440, height: 45 };
  const colored = count(frame, json, chromatic);
  requirePaint(
    colored > 150,
    `JSON fence has insufficient syntax-colored paint (${colored})`,
  );
  const neutral = count(frame, unknown, chromatic);
  requirePaint(
    neutral < 35,
    `unknown fence has syntax-colored paint (${neutral})`,
  );
  const background = pixel(
    frame,
    Math.round(x + 380),
    Math.round(unknown.y + 5),
  );
  const ink = count(frame, unknown, (rgb) =>
    rgb.some((v, i) => Math.abs(v - background[i]!) > 35),
  );
  requirePaint(ink > 100, `unknown fence lacks neutral glyph paint (${ink})`);
}

export function assertLongPaint(
  before: Frame,
  after: Frame,
  observed: unknown,
): void {
  requirePaint(
    before.width === after.width && before.height === after.height,
    'long-code frame dimensions differ',
  );
  const b = bounds(observed);
  const row = roi(before, {
    x: b.x * 2 + 60,
    y: b.y * 2 + 220,
    width: 1300,
    height: 45,
  });
  let changed = 0;
  for (let y = row.y; y < row.y + row.height; y++)
    for (let x = row.x; x < row.x + row.width; x++) {
      const a = pixel(before, x, y),
        z = pixel(after, x, y);
      if (a.some((v, i) => Math.abs(v - z[i]!) > 35)) changed++;
    }
  requirePaint(
    changed > 500,
    `horizontal wheel did not change code row paint (${changed})`,
  );
  // The copy action is outside the scrolling viewport; changing its paint cannot satisfy the row test.
  const action = roi(before, {
    x: 1545,
    y: b.y * 2 + 220,
    width: 145,
    height: 45,
  });
  let actionChanged = 0;
  for (let y = action.y; y < action.y + action.height; y++)
    for (let x = action.x; x < action.x + action.width; x++)
      if (
        pixel(before, x, y).some(
          (v, i) => Math.abs(v - pixel(after, x, y)[i]!) > 35,
        )
      )
        actionChanged++;
  requirePaint(
    actionChanged < 100,
    `copy action moved with scrolled code (${actionChanged})`,
  );
}

export function assertImagePaint(
  frame: Frame,
  fixture: Frame,
  observed: unknown,
): void {
  const b = bounds(observed);
  requirePaint(
    fixture.width === 320 && fixture.height === 180,
    'unexpected bundled image dimensions',
  );
  requirePaint(
    Math.abs(b.width - 640) < 2 && Math.abs(b.height - 360) < 2,
    'unexpected displayed image dimensions',
  );
  roi(frame, {
    x: b.x * 2,
    y: b.y * 2,
    width: b.width * 2,
    height: b.height * 2,
  });
  // Interior gradient points avoid the image border and filtering at the edges.
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
    const expected = pixel(fixture, x!, y!);
    const actual = pixel(
      frame,
      Math.round((b.x + ((x! + 0.5) * b.width) / 320) * 2),
      Math.round((b.y + ((y! + 0.5) * b.height) / 180) * 2),
    );
    requirePaint(
      actual.every((v, i) => Math.abs(v - expected[i]!) <= 16),
      `native image pixel mismatch at fixture (${x}, ${y}): ${actual} vs ${expected}`,
    );
  }
}

export async function assertProbePaint(
  base: string,
  id: string,
): Promise<void> {
  if (id !== 'markdown' && id !== 'files') return; // Other probes have no fixture-specific paint oracle yet.
  const fixture =
    id === 'files'
      ? PNG.sync.read(
          await readFile(
            resolve(import.meta.dir, '../assets/probes/files/landscape.png'),
          ),
        )
      : undefined;
  for (const theme of ['light', 'dark']) {
    const dir = join(base, id, theme);
    const checks = JSON.parse(
      await readFile(join(dir, 'checks.json'), 'utf8'),
    ) as Check[];
    const get = async (
      name: string,
    ): Promise<{ frame: Frame; observed: Record<string, unknown> }> => {
      const check = checks.find((c) => c.name === name);
      requirePaint(
        !!check && check.passed && check.screenshot === `${name}.png`,
        `${id}/${theme}: missing passing ${name} check`,
      );
      const frame = PNG.sync.read(await readFile(join(dir, check.screenshot)));
      requirePaint(
        frame.width === 1800 && frame.height === 1400,
        `${id}/${theme}/${name}: unexpected capture scale`,
      );
      return { frame, observed: check.observations };
    };
    if (id === 'markdown') {
      const mixed = await get('mixed-code');
      assertMixedPaint(mixed.frame, mixed.observed.bounds);
      const before = await get('long-code');
      const after = await get('long-scrolled');
      assertLongPaint(before.frame, after.frame, before.observed.bounds);
    } else {
      const image = await get('image-ready');
      assertImagePaint(image.frame, fixture!, image.observed.image_bounds);
    }
  }
}
