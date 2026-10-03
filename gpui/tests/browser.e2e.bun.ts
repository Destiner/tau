import { expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { compare, pixelMetrics, roi } from '../scripts/compare';
import type { Capture, Suite } from '../contract';

const one = (id: string): Suite => ({
  ...suite('light'),
  scenarios: suite('light').scenarios.filter((s) => s.id === id),
});
const dir = (): Promise<string> => mkdtemp(join(tmpdir(), 'tau-browser-e2e-'));

async function image(dir: string, capture: Capture): Promise<PNG> {
  return PNG.sync.read(await readFile(join(dir, capture.screenshot)));
}

function interiorDifference(
  a: PNG,
  b: PNG,
  bounds: Capture['bounds'],
  scale: number,
): { count: number; minX: number; maxX: number; minY: number; maxY: number } {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  let count = 0;
  let minX = a.width,
    maxX = -1,
    minY = a.height,
    maxY = -1;
  for (
    let y = Math.ceil((bounds.y + 5) * scale);
    y < Math.floor((bounds.y + bounds.height - 5) * scale);
    y++
  )
    for (
      let x = Math.ceil((bounds.x + 5) * scale);
      x < Math.floor((bounds.x + bounds.width - 5) * scale);
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

test('real browser caret is painted at insertion boundaries and in an empty placeholder', async () => {
  const root = await dir();
  try {
    for (const theme of ['light', 'dark'] as const) {
      const s = {
        ...suite(theme),
        scenarios: [
          one('input-caret-boundaries').scenarios[0]!,
          structuredClone(one('input-placeholder-caret').scenarios[0]!),
        ],
      };
      const placeholder = s.scenarios[1]!;
      const visible = placeholder.steps.at(-1)!;
      if (visible.action !== 'checkpoint')
        throw new Error('missing placeholder checkpoint');
      placeholder.steps.splice(-1, 0, {
        ...visible,
        name: 'hidden',
        caret: undefined,
      });
      const out = join(root, theme);
      const frames = await runBrowser(s, out, 'chromium');
      const frame = (id: string, name: string): Capture =>
        frames.find((c) => c.scenario === id && c.checkpoint === name)!;
      for (const id of ['input-caret-boundaries', 'input-placeholder-caret']) {
        const hidden = frame(id, 'hidden');
        expect(hidden.caret ?? 'hidden').toBe('hidden');
        const baseline = await image(out, hidden);
        let priorX = -1;
        for (const name of id === 'input-caret-boundaries'
          ? ['latin', 'before-emoji', 'after-emoji', 'end']
          : ['visible']) {
          const shown = frame(id, name);
          expect(shown.caret).toBe('visible');
          if (name === 'latin' || id === 'input-placeholder-caret')
            expect(shown.observation).toEqual(hidden.observation);
          const delta = interiorDifference(
            baseline,
            await image(out, shown),
            shown.bounds,
            s.scale,
          );
          expect(
            delta.count,
            `${theme}/${id}/${name}: real browser caret must paint`,
          ).toBeGreaterThan(12);
          expect(
            delta.maxX - delta.minX + 1,
            `${theme}/${id}/${name}: localized caret stroke`,
          ).toBeLessThanOrEqual(4);
          expect(
            delta.maxY - delta.minY,
            `${theme}/${id}/${name}: vertical caret stroke`,
          ).toBeGreaterThan(12);
          expect(
            delta.minX,
            `${theme}/${id}/${name}: caret follows insertion boundaries`,
          ).toBeGreaterThan(priorX);
          priorX = delta.minX;
        }
      }
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);

test('real browser disabled selection stays painted and disabled primary button does not activate', async () => {
  const root = await dir();
  try {
    for (const theme of ['light', 'dark'] as const) {
      const s: Suite = {
        ...suite(theme),
        scenarios: [
          structuredClone(one('input-tab-focus').scenarios[0]!),
          structuredClone(one('button-primary-disabled').scenarios[0]!),
          structuredClone(one('button-primary-md').scenarios[0]!),
        ],
      };
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
      const out = join(root, theme);
      const frames = await runBrowser(s, out, 'chromium');
      const frame = (id: string, name: string): Capture =>
        frames.find((c) => c.scenario === id && c.checkpoint === name)!;
      const selected = frame('input-tab-focus', 'disabled-blur');
      expect(selected.observation).toMatchObject({
        value: 'Tab',
        focused: false,
        disabled: true,
        selection: [0, 3],
      });
      expect(frame('input-tab-focus', 'skipped').observation).toMatchObject({
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
      const delta = interiorDifference(
        await image(out, selected),
        await image(out, frame(collapsed.id, 'disabled')),
        selected.bounds,
        s.scale,
      );
      expect(
        delta.count,
        `${theme}: disabled selection highlight must remain visible`,
      ).toBeGreaterThan(30);
      const button = frame('button-primary-disabled', 'idle');
      expect(
        frame('button-primary-disabled', 'click').observation,
      ).toMatchObject({ clicks: 0, disabled: true });
      const png = await image(out, button);
      const y = Math.round(
        (button.bounds.y + button.bounds.height / 2) * s.scale,
      );
      const left = Math.round(button.bounds.x * s.scale);
      const color = (x: number): number[] => [
        ...png.data.subarray(
          (y * png.width + x) * 4,
          (y * png.width + x) * 4 + 3,
        ),
      ];
      const fill = color(
        Math.round((button.bounds.x + button.bounds.width) * s.scale) - 12,
      );
      const border = color(left + 1);
      const canvas = [...png.data.subarray(0, 3)];
      const enabled = frame('button-primary-md', 'idle');
      const enabledImage = await image(out, enabled);
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
        `${theme}: disabled primary border must match its pale fill`,
      ).toBeLessThan(16);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);

test('real controls: deterministic captures, reset isolation, and visible changes without state changes', async () => {
  const root = await dir();
  try {
    const s = one('input-seeded');
    s.scenarios.push(one('input-empty').scenarios[0]!);
    const baseline = await runBrowser(s, join(root, 'baseline'), 'chromium');
    const repeated = await runBrowser(s, join(root, 'repeated'), 'chromium');
    expect(repeated.map((c) => c.observation)).toEqual(
      baseline.map((c) => c.observation),
    );
    expect(repeated[1]!.observation.value).toBe('');
    const isolated = one('input-type');
    isolated.scenarios.push(one('input-fresh-undo').scenarios[0]!);
    const isolation = await runBrowser(
      isolated,
      join(root, 'isolation'),
      'chromium',
    );
    expect(isolation.at(-1)!.observation).toMatchObject({
      value: 'fresh',
      changes: 0,
    });
    const image = 'input-seeded--idle.png';
    expect(await readFile(join(root, 'baseline', image))).toEqual(
      await readFile(join(root, 'repeated', image)),
    );
    const changed = await runBrowser(
      s,
      join(root, 'changed'),
      'chromium',
      async (page) => {
        await page.addStyleTag({
          content: '#subject-wrap input { color: transparent !important }',
        });
      },
    );
    expect(changed.map((c) => c.observation)).toEqual(
      baseline.map((c) => c.observation),
    );
    const a = PNG.sync.read(await readFile(join(root, 'baseline', image)));
    const b = PNG.sync.read(await readFile(join(root, 'changed', image)));
    expect(
      pixelMetrics(
        a,
        b,
        roi(
          baseline[0]!.bounds,
          changed[0]!.bounds,
          a.width,
          a.height,
          s.scale,
        ),
      ).differingPixels,
    ).toBeGreaterThan(0);
    await expect(
      compare(
        s,
        baseline,
        changed,
        join(root, 'baseline'),
        join(root, 'changed'),
        join(root, 'report'),
        true,
      ),
    ).rejects.toThrow('visual review');
    await rm(join(root, 'changed', image));
    await expect(
      compare(
        s,
        baseline,
        changed,
        join(root, 'baseline'),
        join(root, 'changed'),
        join(root, 'report'),
      ),
    ).rejects.toThrow('ENOENT');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);

test('real browser rejects behavioral faults and records diagnostic observations', async () => {
  const root = await dir();
  try {
    await expect(
      runBrowser(
        one('button-keyboard'),
        join(root, 'handler'),
        'chromium',
        async (page) => {
          await page.evaluate(() =>
            document.addEventListener(
              'click',
              (event) => event.stopImmediatePropagation(),
              true,
            ),
          );
        },
      ),
    ).rejects.toThrow('Vue reference');
    expect(
      await readFile(join(root, 'handler', 'report.json'), 'utf8'),
    ).toContain('clicks: expected 1');
    await runBrowser(one('input-empty'), join(root, 'handler'), 'chromium');
    await expect(
      readFile(join(root, 'handler', 'failure.json')),
    ).rejects.toThrow('ENOENT');
    await expect(
      runBrowser(
        one('input-insert'),
        join(root, 'caret'),
        'chromium',
        async (page) => {
          await page.evaluate(() => {
            window.tauReference.select = (): void => {};
          });
        },
      ),
    ).rejects.toThrow('value: expected');
    await expect(
      runBrowser(
        one('input-empty'),
        join(root, 'load'),
        'chromium',
        async (page) => {
          await page.goto('http://127.0.0.1:1/unavailable');
        },
      ),
    ).rejects.toThrow();
    expect(
      await readFile(join(root, 'load', 'failure.json'), 'utf8'),
    ).toContain('error');
    await expect(
      runBrowser(
        one('input-empty'),
        join(root, 'console'),
        'chromium',
        async (page) => {
          await page.evaluate(() => console.error('fixture failure'));
        },
      ),
    ).rejects.toThrow('fixture failure');
    await expect(
      runBrowser(
        one('input-empty'),
        join(root, 'external'),
        'chromium',
        async (page) => {
          await page.evaluate(() => {
            const image = document.createElement('img');
            image.src = 'https://example.com/forbidden.png';
            document.body.append(image);
          });
        },
      ),
    ).rejects.toThrow('blocked external requests');
    expect(
      await readFile(join(root, 'external', 'report.json'), 'utf8'),
    ).toContain('example.com');
    await expect(
      runBrowser(
        one('input-empty'),
        join(root, 'socket'),
        'chromium',
        async (page) => {
          await page.evaluate(
            () =>
              new Promise<void>((resolve) => {
                const socket = new WebSocket('wss://example.invalid/forbidden');
                socket.onclose = (): void => resolve();
              }),
          );
        },
      ),
    ).rejects.toThrow('blocked external requests');
    const malformed = one('input-empty');
    (malformed as unknown as { scale: number }).scale = 1;
    await expect(
      runBrowser(malformed, join(root, 'bad'), 'chromium'),
    ).rejects.toThrow('scale');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}, 120000);
