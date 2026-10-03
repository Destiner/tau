import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import { compare } from '../scripts/compare';
import { renderReport } from '../scripts/report';
import type { Capture, Suite } from '../contract';

test('report escapes untrusted text and keeps image links relative', () => {
  const suite = {
    theme: '<script>alert(1)</script>',
    scale: 2,
  } as unknown as Suite;
  const row = {
    id: 'scenario/<img src=x>',
    state: ['<bad>'],
    review: true,
    visual: {},
    browser: '../vue/image.png',
    native: '../native/image.png',
    browserCrop: 'vue--image.png',
    nativeCrop: 'gpui--image.png',
    diff: 'image.png',
    cropWidth: 100,
    cropHeight: 40,
  } as unknown as import('../scripts/compare').Row;
  const html = renderReport(suite, [row]);
  expect(html).toContain('&lt;script&gt;');
  expect(html).toContain('&lt;img src=x&gt;');
  expect(html).toContain('&lt;bad&gt;');
  expect(html).not.toContain('<bad>');
  expect(html).toContain('data-id="scenario/&lt;img src=x&gt;"');
  expect(html).toContain('width="50" height="20"');
});

test('state and visual faults are reported separately; strict visuals fail without approval', async () => {
  const root = await mkdtemp(join(tmpdir(), 'tau-compare-'));
  const vue = join(root, 'vue'),
    native = join(root, 'native'),
    report = join(root, 'report');
  await mkdir(vue);
  await mkdir(native);
  const s: Suite = {
    schemaVersion: 1,
    theme: 'light',
    viewport: { width: 640, height: 320 },
    scale: 2,
    scenarios: [
      {
        id: 'fault',
        component: 'button',
        props: {},
        steps: [
          { action: 'checkpoint', name: 'idle', expected: { clicks: 0 } },
        ],
      },
    ],
  };
  const c: Capture = {
    scenario: 'fault',
    checkpoint: 'idle',
    screenshot: 'fault--idle.png',
    bounds: { x: 24, y: 24, width: 48, height: 22 },
    observation: {
      value: 'Continue',
      selection: null,
      focused: false,
      disabled: false,
      readonly: false,
      clicks: 0,
      changes: 0,
    },
  };
  const png = new PNG({ width: 1280, height: 640 });
  png.data.fill(255);
  await writeFile(join(vue, c.screenshot), PNG.sync.write(png));
  png.data[(48 * 1280 + 48) * 4] = 0;
  await writeFile(join(native, c.screenshot), PNG.sync.write(png));
  const rows = await compare(s, [c], [c], vue, native, report);
  expect(rows[0]!.visual.roiPixels).toBe((48 + 8) * (22 + 8) * 4);
  const crop = PNG.sync.read(
    await readFile(join(report, rows[0]!.browserCrop)),
  );
  expect([crop.width, crop.height]).toEqual([112, 60]);
  const html = await readFile(join(report, 'report.html'), 'utf8');
  expect(html).toContain('width="56" height="30"');
  expect(html).toContain('href="../vue/fault--idle.png"');
  expect(html).toContain('href="../native/fault--idle.png"');
  expect(html).toContain('href="full--fault--idle.png"');
  expect(html).toContain('id="filter"');
  expect(html).toContain('id="failures"');
  expect(html).toContain('id="zoom"');
  expect(rows[0]!.state).toEqual([]);
  expect(rows[0]!.visual.differingPixels).toBe(1);
  expect(rows[0]!.review).toBe(true);
  expect(rows[0]!.visual.fullFrameDifferingPixels).toBe(1);
  expect(rows[0]!.visual.fullFramePixels).toBe(1280 * 640);
  expect(rows[0]!.visual.fraction).toBeGreaterThan(
    rows[0]!.visual.fullFrameFraction,
  );
  const outside = new PNG({ width: 1280, height: 640 });
  outside.data.fill(255);
  outside.data[(300 * 1280 + 900) * 4] = 0;
  await writeFile(join(native, c.screenshot), PNG.sync.write(outside));
  const outsideRows = await compare(s, [c], [c], vue, native, report);
  expect(outsideRows[0]!.visual.differingPixels).toBe(0);
  expect(outsideRows[0]!.visual.fullFrameDifferingPixels).toBe(1);
  expect(outsideRows[0]!.review).toBe(true);
  await expect(compare(s, [c], [c], vue, native, report, true)).rejects.toThrow(
    'visual review',
  );
  expect(await readFile(join(report, 'report.html'), 'utf8')).toContain(
    'VISUAL REVIEW',
  );
  await expect(compare(s, [c], [c], vue, native, report, true)).rejects.toThrow(
    'visual review',
  );
  await expect(
    compare(
      s,
      [c],
      [{ ...c, observation: { ...c.observation, clicks: 2 } }],
      vue,
      native,
      report,
    ),
  ).rejects.toThrow('state failures');
  expect(await readFile(join(report, 'report.json'), 'utf8')).toContain(
    'GPUI vs expected',
  );
  await writeFile(
    join(native, c.screenshot),
    PNG.sync.write(new PNG({ width: 640, height: 320 })),
  );
  await expect(compare(s, [c], [c], vue, native, report)).rejects.toThrow(
    'expected 1280x640',
  );
});
