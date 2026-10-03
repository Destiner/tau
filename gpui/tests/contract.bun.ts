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
import { pixelMetrics, roi } from '../scripts/compare';
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
test('CLI rejects unknown options and components', () => {
  expect(options(['--reference-only', '--browser', 'webkit']).browser).toBe(
    'webkit',
  );
  expect(() => options(['--component', 'checkbox'])).toThrow(
    'unknown component',
  );
  expect(() => options(['--bad'])).toThrow('unknown');
});
