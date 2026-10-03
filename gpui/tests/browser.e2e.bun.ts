import { expect, test } from 'bun:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PNG } from 'pngjs';
import { suite } from '../scenarios';
import { runBrowser } from '../scripts/browser';
import { compare, pixelMetrics, roi } from '../scripts/compare';
import type { Suite } from '../contract';

const one = (id: string): Suite => ({
  ...suite('light'),
  scenarios: suite('light').scenarios.filter((s) => s.id === id),
});
const dir = (): Promise<string> => mkdtemp(join(tmpdir(), 'tau-browser-e2e-'));

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
