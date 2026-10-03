import { test, expect } from 'bun:test';
import { PNG } from 'pngjs';
import {
  assertCaptures,
  stateDifferences,
  validateSuite,
  validateSelection,
  type Capture,
  type Suite,
} from '../contract';
import { scenarios, suite } from '../scenarios';
import { diagnosticTranslation, pixelMetrics, roi } from '../scripts/compare';
import { options } from '../scripts/run';

test('suite has authored checkpoints and distinct coverage without count quotas', () => {
  for (const id of [
    'button-keyboard',
    'button-cancelled-press',
    'input-pointer-selection',
    'input-grapheme-navigation',
    'input-undo-redo',
    'textarea-wrapped-rows',
  ])
    expect(scenarios.some((scenario) => scenario.id === id)).toBe(true);
  for (const theme of ['light', 'dark'] as const) validateSuite(suite(theme));
  expect(new Set(scenarios.map((s) => s.id)).size).toBe(scenarios.length);
});
test('rejects unknown action, props, empty expectation and duplicate ids', () => {
  const base = (): Suite => structuredClone(suite('light')) as Suite;
  const s = base();
  s.scenarios[0]!.steps = [{ action: 'checkpoint', name: 'x', expected: {} }];
  expect(() => validateSuite(s)).toThrow('empty expected');
  const t = base();
  t.scenarios[0]!.steps[0] = { action: 'scroll' } as never;
  expect(() => validateSuite(t)).toThrow('unknown action');
  const u = base();
  (u.scenarios[0]!.props as Record<string, unknown>).password = true;
  expect(() => validateSuite(u)).toThrow('unknown key');
  const v = base();
  v.scenarios[1]!.id = v.scenarios[0]!.id;
  expect(() => validateSuite(v)).toThrow('duplicate');
  const w = base();
  (w as unknown as Record<string, unknown>).scale = 1;
  expect(() => validateSuite(w)).toThrow('scale');
});
test('UTF-16 selection, partial expected state, and capture completeness', () => {
  const s: Suite = {
    schemaVersion: 1,
    theme: 'light',
    viewport: { width: 640, height: 320 },
    scale: 2,
    scenarios: [
      {
        id: 'emoji',
        component: 'input',
        props: { value: 'A😀B' },
        steps: [
          {
            action: 'checkpoint',
            name: 'selection',
            expected: { selection: [1, 3], changes: 0 },
          },
        ],
      },
    ],
  };
  validateSuite(s);
  const c: Capture = {
    scenario: 'emoji',
    checkpoint: 'selection',
    observation: {
      value: 'A😀B',
      selection: [1, 3],
      focused: false,
      disabled: false,
      readonly: false,
      clicks: 0,
      changes: 0,
    },
    bounds: { x: 24, y: 24, width: 360, height: 28 },
    screenshot: 'emoji--selection.png',
  };
  expect(stateDifferences({ selection: [1, 3] }, c.observation)).toEqual([]);
  expect(stateDifferences({ selection: [1, 2] }, c.observation)).toHaveLength(
    1,
  );
  expect(() => assertCaptures(s, [], 'native')).toThrow('missing capture');
  expect(() => assertCaptures(s, [c, c], 'native')).toThrow('duplicate');
  expect(() =>
    assertCaptures(s, [{ ...c, scenario: 'unknown' }], 'native'),
  ).toThrow('unknown');
  expect(() => assertCaptures(s, [c], 'native')).not.toThrow();
  expect(() =>
    assertCaptures(s, [{ ...c, bounds: { ...c.bounds, x: -1 } }], 'native'),
  ).toThrow('invalid bounds');
  expect(() =>
    assertCaptures(
      s,
      [{ ...c, bounds: { ...c.bounds, width: 640 } }],
      'native',
    ),
  ).toThrow('invalid bounds');
  expect(() => validateSelection('A😀B', 1, 3)).not.toThrow();
  expect(() => validateSelection('A😀B', 2, 2)).toThrow('scalar boundaries');
  expect(() => validateSelection('A😀B', 9, 9)).toThrow('scalar boundaries');
  expect(() => validateSelection('A😀B', 3, 1)).toThrow('scalar boundaries');
});
test('ROI excludes canvas and deliberate visual fault registers pixels', () => {
  const a = new PNG({ width: 1280, height: 640, fill: true });
  a.data.fill(255);
  const b = PNG.sync.read(PNG.sync.write(a));
  const region = roi(
    { x: 24, y: 24, width: 22, height: 28 },
    { x: 25, y: 24, width: 22, height: 28 },
    1280,
    640,
    2,
  );
  expect(region).toEqual({ x: 40, y: 40, width: 62, height: 72 });
  b.data[(48 * 1280 + 48) * 4] = 0;
  expect(pixelMetrics(a, b, region).differingPixels).toBe(1);
  expect(() =>
    pixelMetrics(a, new PNG({ width: 32, height: 32 }), region),
  ).toThrow('render size mismatch');
});
test('magnitude is diagnostic, not a tolerance: rounding, subtle fills and invisible controls remain exact', () => {
  const a = new PNG({ width: 24, height: 24 });
  a.data.fill(255);
  const b = PNG.sync.read(PNG.sync.write(a));
  const region = { x: 0, y: 0, width: 24, height: 24 };
  for (let y = 4; y < 20; y++)
    for (let x = 4; x < 20; x++) b.data[(y * 24 + x) * 4] = 254;
  const subtle = pixelMetrics(a, b, region);
  expect(subtle.differingPixels).toBe(256);
  expect(subtle.minorPixels).toBe(256);
  expect(subtle.majorPixels).toBe(0);
  expect(subtle.heatmap.data[(5 * 24 + 5) * 4 + 1]).toBeLessThan(255);
  for (let y = 4; y < 20; y++)
    for (let x = 4; x < 20; x++) b.data[(y * 24 + x) * 4] = 0;
  const missing = pixelMetrics(a, b, region);
  expect(missing.majorPixels).toBe(256);
  expect(missing.differingPixels).toBe(256);
  expect(missing.heatmap.data[(5 * 24 + 5) * 4 + 1]).toBeLessThan(
    subtle.heatmap.data[(5 * 24 + 5) * 4 + 1]!,
  );
});
test('diagnostic shift detects real position errors without changing raw metrics', () => {
  const a = new PNG({ width: 32, height: 24 });
  a.data.fill(255);
  const b = PNG.sync.read(PNG.sync.write(a));
  for (let y = 7; y < 15; y++)
    for (let x = 8; x < 12; x++) {
      a.data[(y * 32 + x) * 4] = 0;
      b.data[(y * 32 + x + 1) * 4] = 0;
    }
  const region = { x: 2, y: 2, width: 26, height: 18 };
  const raw = pixelMetrics(a, b, region);
  expect(raw.differingPixels).toBe(16);
  expect(diagnosticTranslation(a, b, region)).toEqual({
    dx: 1,
    dy: 0,
    errorReductionPercent: 100,
  });
  expect(pixelMetrics(a, b, region).differingPixels).toBe(16);
});
test('exact RGBA includes alpha changes and identical frames do not invent a shift', () => {
  const a = new PNG({ width: 8, height: 8 });
  a.data.fill(255);
  const b = PNG.sync.read(PNG.sync.write(a));
  const region = { x: 0, y: 0, width: 8, height: 8 };
  expect(diagnosticTranslation(a, b, region)).toEqual({
    dx: 0,
    dy: 0,
    errorReductionPercent: 0,
  });
  b.data[3] = 254;
  const metrics = pixelMetrics(a, b, region);
  expect(metrics.differingPixels).toBe(1);
  expect(metrics.minorPixels).toBe(1);
  expect([...metrics.diff.data.subarray(0, 4)]).toEqual([255, 0, 0, 255]);
});

test('CLI rejects unknown options and components', () => {
  expect(options(['--reference-only', '--browser', 'webkit']).browser).toBe(
    'webkit',
  );
  expect(() => options(['--component', 'checkbox'])).toThrow(
    'unknown component',
  );
  expect(() => options(['--bad'])).toThrow('unknown');
});
