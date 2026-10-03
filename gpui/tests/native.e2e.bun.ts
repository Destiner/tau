import { beforeAll, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import {
  assertCaptures,
  stateDifferences,
  type Capture,
  type Suite,
} from '../contract';
import { compare } from '../scripts/compare';
import { prepare } from '../scripts/prepare';
import { exec, native, pick } from '../scripts/native-test-helper';

beforeAll(async () => {
  await prepare();
  const build = await exec('cargo', ['build', '--locked']);
  if (build.code !== 0) throw new Error(`native build failed: ${build.output}`);
}, 600000);

async function workspace(run: (root: string) => Promise<void>): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), 'tau-native-e2e-'));
  try {
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

async function capture(
  s: Suite,
  dir: string,
  fault?: string,
): Promise<Capture[]> {
  const result = await native(s, dir, fault);
  if (result.code !== 0 || !result.captures)
    throw new Error(`Metal capture failed: ${result.output}`);
  assertCaptures(s, result.captures, 'native');
  return result.captures;
}

async function image(dir: string, capture: Capture): Promise<PNG> {
  return PNG.sync.read(await readFile(join(dir, capture.screenshot)));
}

function changedInterior(
  a: PNG,
  b: PNG,
  bounds: Capture['bounds'],
  scale: number,
  inset = 5,
): { count: number; minX: number; maxX: number; minY: number; maxY: number } {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  let count = 0;
  let minX = a.width,
    maxX = -1,
    minY = a.height,
    maxY = -1;
  for (
    let y = Math.ceil((bounds.y + inset) * scale);
    y < Math.floor((bounds.y + bounds.height - inset) * scale);
    y++
  )
    for (
      let x = Math.ceil((bounds.x + inset) * scale);
      x < Math.floor((bounds.x + bounds.width - inset) * scale);
      x++
    ) {
      const i = (y * a.width + x) * 4;
      if (
        a.data[i] === b.data[i] &&
        a.data[i + 1] === b.data[i + 1] &&
        a.data[i + 2] === b.data[i + 2]
      )
        continue;
      count++;
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
  return { count, minX, maxX, minY, maxY };
}

function expectStates(
  s: Suite,
  captures: Awaited<ReturnType<typeof capture>>,
): void {
  for (const scenario of s.scenarios)
    for (const step of scenario.steps) {
      if (step.action !== 'checkpoint') continue;
      const c = captures.find(
        (entry) =>
          entry.scenario === scenario.id && entry.checkpoint === step.name,
      )!;
      expect(stateDifferences(step.expected, c.observation)).toEqual([]);
    }
}

test(
  'real Metal: repeated frames, state expectations, reset and undo isolation',
  async () =>
    workspace(async (root) => {
      for (const theme of ['light', 'dark'] as const) {
        const s = pick(
          theme,
          'button-keyboard',
          'button-cancelled-press',
          'input-pointer-selection',
          'textarea-pointer-selection',
          'input-type',
          'input-vertical-arrows',
          'input-vertical-selection',
          'input-caret-boundaries',
          'input-placeholder-caret',
          'input-undo-group',
          'input-joined-emoji-navigation',
          'input-emoji',
          'input-selection-anchor',
          'input-fresh-undo',
          'input-tab-focus',
          'input-readonly',
          'input-disabled',
          'textarea-emoji',
          'textarea-readonly',
          'textarea-disabled',
          'textarea-wrapped-rows',
        );
        const first = join(root, `${theme}-first`),
          second = join(root, `${theme}-second`);
        const a = await capture(s, first),
          b = await capture(s, second);
        expectStates(s, a);
        expectStates(s, b);
        expect(b.map((c) => c.observation)).toEqual(
          a.map((c) => c.observation),
        );
        const fresh = b.find((c) => c.scenario === 'input-fresh-undo')!;
        expect(fresh.observation).toMatchObject({ value: 'fresh', changes: 0 });
        const rows = await compare(
          s,
          a,
          b,
          first,
          second,
          join(root, `${theme}-report`),
          true,
        );
        expect(rows.every((row) => !row.review && row.state.length === 0)).toBe(
          true,
        );
        for (const c of a)
          expect(await readFile(join(first, c.screenshot))).toEqual(
            await readFile(join(second, c.screenshot)),
          );
      }
    }),
  180000,
);

test(
  'text selection paints inside both editors, not just in observations',
  async () =>
    workspace(async (root) => {
      for (const theme of ['light', 'dark'] as const) {
        const s = pick(theme, 'input-pointer-selection');
        s.scenarios = (['input', 'textarea'] as const).map((component) => ({
          id: `${component}-selection-paint`,
          component,
          props: { value: 'abcdefghij', monospaced: true },
          steps: [
            { action: 'focus' },
            {
              action: 'checkpoint',
              name: 'caret',
              expected: {
                value: 'abcdefghij',
                focused: true,
                selection: [10, 10],
              },
            },
            { action: 'key', key: 'Shift+ArrowLeft' },
            { action: 'key', key: 'Shift+ArrowLeft' },
            {
              action: 'checkpoint',
              name: 'selected',
              expected: {
                value: 'abcdefghij',
                focused: true,
                selection: [8, 10],
              },
            },
          ],
        }));
        const dir = join(root, theme);
        const frames = await capture(s, dir);
        expectStates(s, frames);
        for (const scenario of s.scenarios) {
          const before = frames.find(
            (c) => c.scenario === scenario.id && c.checkpoint === 'caret',
          )!;
          const after = frames.find(
            (c) => c.scenario === scenario.id && c.checkpoint === 'selected',
          )!;
          const a = PNG.sync.read(await readFile(join(dir, before.screenshot)));
          const b = PNG.sync.read(await readFile(join(dir, after.screenshot)));
          const bounds = before.bounds;
          let changed = 0;
          // Ignore the focus border; the value, caret suppression and styling are unchanged.
          for (
            let y = Math.ceil((bounds.y + 4) * s.scale);
            y < (bounds.y + bounds.height - 4) * s.scale;
            y++
          ) {
            for (
              let x = Math.ceil((bounds.x + 8) * s.scale);
              x < (bounds.x + bounds.width - 8) * s.scale;
              x++
            ) {
              const offset = (y * a.width + x) * 4;
              if (
                a.data[offset] !== b.data[offset] ||
                a.data[offset + 1] !== b.data[offset + 1] ||
                a.data[offset + 2] !== b.data[offset + 2]
              )
                changed++;
            }
          }
          expect(
            changed,
            `${theme}/${scenario.id}: selection highlight must be visible`,
          ).toBeGreaterThan(200);
        }
      }
    }),
  60000,
);

test(
  'real Metal caret is a narrow centered painted insertion mark, including empty placeholder',
  async () =>
    workspace(async (root) => {
      for (const theme of ['light', 'dark'] as const) {
        const s = pick(
          theme,
          'input-caret-boundaries',
          'input-placeholder-caret',
        );
        const placeholder = s.scenarios[1]!;
        const visible = placeholder.steps.at(-1)!;
        if (visible.action !== 'checkpoint')
          throw new Error('missing placeholder checkpoint');
        placeholder.steps.splice(-1, 0, {
          ...visible,
          name: 'hidden',
          caret: undefined,
        });
        const dir = join(root, theme);
        const frames = await capture(s, dir);
        expectStates(s, frames);
        const frame = (scenario: string, checkpoint: string): Capture =>
          frames.find(
            (c) => c.scenario === scenario && c.checkpoint === checkpoint,
          )!;
        for (const id of [
          'input-caret-boundaries',
          'input-placeholder-caret',
        ]) {
          const hidden = frame(id, 'hidden');
          expect(hidden.caret ?? 'hidden').toBe('hidden');
          const hiddenImage = await image(dir, hidden);
          let priorX = -1;
          for (const name of id === 'input-caret-boundaries'
            ? ['latin', 'before-emoji', 'after-emoji', 'end']
            : ['visible']) {
            const shown = frame(id, name);
            expect(shown.caret).toBe('visible');
            if (name === 'latin' || id === 'input-placeholder-caret')
              expect(shown.observation).toEqual(hidden.observation);
            const delta = changedInterior(
              hiddenImage,
              await image(dir, shown),
              shown.bounds,
              s.scale,
            );
            expect(
              delta.count,
              `${theme}/${id}/${name}: actual caret pixels`,
            ).toBeGreaterThan(12);
            expect(
              delta.maxX - delta.minX + 1,
              `${theme}/${id}/${name}: one logical pixel at insertion`,
            ).toBeLessThanOrEqual(3);
            expect(
              delta.maxY - delta.minY,
              `${theme}/${id}/${name}: vertical caret stroke`,
            ).toBeGreaterThan(12);
            expect(
              delta.minX,
              `${theme}/${id}/${name}: caret follows UTF-16 scalar boundaries`,
            ).toBeGreaterThan(priorX);
            priorX = delta.minX;
          }
        }
      }
    }),
  60000,
);

test(
  'real Metal retains disabled selection paint and primary button has a single pale border',
  async () =>
    workspace(async (root) => {
      for (const theme of ['light', 'dark'] as const) {
        const s = pick(
          theme,
          'input-tab-focus',
          'button-primary-disabled',
          'button-primary-md',
        );
        const tab = s.scenarios[0]!;
        const reenable = tab.steps.findIndex(
          (step) => step.action === 'props' && step.props.disabled === false,
        );
        tab.steps.splice(
          reenable,
          0,
          { action: 'type', text: 'X' },
          {
            action: 'checkpoint',
            name: 'typing-blocked',
            expected: {
              value: 'Tab',
              changes: 0,
              disabled: true,
              focused: false,
              selection: [0, 3],
            },
          },
        );
        const collapsed = structuredClone(tab);
        collapsed.id = 'input-collapsed-disabled';
        collapsed.steps = [
          { action: 'focus' },
          { action: 'select', start: 3, end: 3 },
          { action: 'props', props: { disabled: true } },
          {
            action: 'checkpoint',
            name: 'disabled',
            expected: {
              value: 'Tab',
              disabled: true,
              focused: false,
              selection: [3, 3],
            },
          },
        ];
        s.scenarios.push(collapsed);
        const dir = join(root, theme);
        const frames = await capture(s, dir);
        expectStates(s, frames);
        const frame = (id: string, name: string): Capture =>
          frames.find((c) => c.scenario === id && c.checkpoint === name)!;
        const selected = frame('input-tab-focus', 'disabled-blur');
        const skipped = frame('input-tab-focus', 'skipped');
        expect(selected.observation).toMatchObject({
          value: 'Tab',
          focused: false,
          disabled: true,
          selection: [0, 3],
        });
        expect(skipped.observation).toMatchObject({
          focused: false,
          disabled: true,
          selection: [0, 3],
        });
        expect(
          frame('input-tab-focus', 'typing-blocked').observation,
        ).toMatchObject({
          value: 'Tab',
          changes: 0,
          disabled: true,
          focused: false,
          selection: [0, 3],
        });
        const delta = changedInterior(
          await image(dir, selected),
          await image(dir, frame(collapsed.id, 'disabled')),
          selected.bounds,
          s.scale,
        );
        expect(
          delta.count,
          `${theme}: disabled selection must remain painted without focus or caret`,
        ).toBeGreaterThan(30);
        const button = frame('button-primary-disabled', 'idle');
        expect(
          frame('button-primary-disabled', 'click').observation,
        ).toMatchObject({ clicks: 0, disabled: true });
        const png = await image(dir, button);
        const y = Math.round(
          (button.bounds.y + button.bounds.height / 2) * s.scale,
        );
        const left = Math.round(button.bounds.x * s.scale);
        const rgb = (x: number): number[] => [
          ...png.data.subarray(
            (y * png.width + x) * 4,
            (y * png.width + x) * 4 + 3,
          ),
        ];
        const fill = rgb(
          Math.round((button.bounds.x + button.bounds.width) * s.scale) - 12,
        );
        const border = rgb(left + 1);
        const canvas = [...png.data.subarray(0, 3)];
        const enabled = frame('button-primary-md', 'idle');
        const enabledImage = await image(dir, enabled);
        const enabledY = Math.round(
          (enabled.bounds.y + enabled.bounds.height / 2) * s.scale,
        );
        const enabledX =
          Math.round((enabled.bounds.x + enabled.bounds.width) * s.scale) - 12;
        const enabledFill = [
          ...enabledImage.data.subarray(
            (enabledY * enabledImage.width + enabledX) * 4,
            (enabledY * enabledImage.width + enabledX) * 4 + 3,
          ),
        ];
        const distance = (color: number[]): number =>
          color.reduce(
            (sum, channel, i) => sum + Math.abs(channel - canvas[i]!),
            0,
          );
        expect(
          distance(fill),
          `${theme}: disabled fill is paler than enabled`,
        ).toBeLessThan(distance(enabledFill));
        expect(
          Math.max(...border.map((channel, i) => Math.abs(channel - fill[i]!))),
          `${theme}: disabled primary border and fill should be equally pale`,
        ).toBeLessThan(16);
      }
    }),
  60000,
);

test(
  'rendering fault keeps observations but strict pixels reject it',
  async () =>
    workspace(async (root) => {
      const s = pick('light', 'button-primary-md');
      const a = await capture(s, join(root, 'normal'));
      const b = await capture(s, join(root, 'hidden'), 'hide-control');
      expect(b.map((c) => c.observation)).toEqual(a.map((c) => c.observation));
      await expect(
        compare(
          s,
          a,
          b,
          join(root, 'normal'),
          join(root, 'hidden'),
          join(root, 'diff'),
          true,
        ),
      ).rejects.toThrow('visual review');
    }),
  60000,
);

test(
  'click and editor Change faults are detected by independent expectations',
  async () =>
    workspace(async (root) => {
      for (const [id, fault, field] of [
        ['button-keyboard', 'drop-clicks', 'clicks'],
        ['input-type', 'drop-changes', 'changes'],
      ] as const) {
        const s = pick('light', id);
        const baseline = await capture(s, join(root, `${id}-baseline`));
        expectStates(s, baseline);
        const faulty = await capture(s, join(root, `${id}-fault`), fault);
        expect(
          faulty.some(
            (c) =>
              c.observation[field] !==
              baseline.find((a) => a.checkpoint === c.checkpoint)!.observation[
                field
              ],
          ),
        ).toBe(true);
        await expect(
          compare(
            s,
            baseline,
            faulty,
            join(root, `${id}-baseline`),
            join(root, `${id}-fault`),
            join(root, `${id}-report`),
          ),
        ).rejects.toThrow('state failures');
      }
    }),
  60000,
);

test(
  'bad requests reject and remove stale manifest',
  async () =>
    workspace(async (root) => {
      const base = pick('light', 'input-emoji');
      const invalid: unknown[] = [];
      const variant = (change: (s: Suite) => void): void => {
        const s = structuredClone(base);
        change(s);
        invalid.push(s);
      };
      variant((s) => {
        s.scale = 3 as 2;
      });
      variant((s) => {
        s.scenarios[0]!.id = '../escape';
      });
      variant((s) => {
        s.scenarios.push(structuredClone(s.scenarios[0]!));
      });
      variant((s) => {
        s.scenarios[0]!.steps.push({ action: 'scroll' } as never);
      });
      variant((s) => {
        s.scenarios[0]!.steps.push({ action: 'select', start: 99, end: 99 });
      });
      variant((s) => {
        s.scenarios[0]!.steps.splice(2, 0, {
          action: 'select',
          start: 2,
          end: 2,
        });
      });
      invalid.push({
        ...base,
        scenarios: [
          {
            ...base.scenarios[0],
            steps: [
              { action: 'checkpoint', name: 'bad', expected: {} },
              { action: 'checkpoint', name: 'bad', expected: {} },
            ],
          },
        ],
      });
      invalid.push('{ malformed');
      variant((s) => {
        s.scenarios[0]!.props.width = -1;
      });
      const dir = join(root, 'reject');
      for (const [i, s] of invalid.entries()) {
        const prior = await native(base, dir);
        expect(prior.code).toBe(0);
        expect(
          await readFile(join(dir, 'observations.json'), 'utf8'),
        ).toContain('input-emoji');
        const request = join(dir, 'request.json');
        if (typeof s === 'string') await writeFile(request, s);
        else await writeFile(request, JSON.stringify(s));
        const result = await exec(
          join(import.meta.dir, '../target/debug/tau-gpui-prototype'),
          ['--suite', request, '--out', dir],
        );
        expect(result.code, `case ${i}: ${result.output}`).not.toBe(0);
        await expect(readFile(join(dir, 'observations.json'))).rejects.toThrow(
          'ENOENT',
        );
      }
      const liveFault = await exec(
        join(import.meta.dir, '../target/debug/tau-gpui-prototype'),
        ['--fault', 'drop-clicks'],
      );
      expect(liveFault.code).not.toBe(0);
    }),
  120000,
);
