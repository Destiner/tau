import { mkdir, readdir, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer as createTcpServer } from 'node:net';
import {
  chromium,
  webkit,
  type BrowserType,
  type Page,
} from '@playwright/test';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';
import { PNG } from 'pngjs';
import {
  assertCaptures,
  stateDifferences,
  validateSuite,
  type Capture,
  type Scenario,
  type Step,
  type Suite,
} from '../contract';

async function pointer(
  page: Page,
  control: ReturnType<Page['locator']>,
  click = true,
): Promise<void> {
  const b = await control.boundingBox();
  if (!b) throw new Error('missing subject');
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  if (click) await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
}
async function action(page: Page, step: Step): Promise<void> {
  const control = page.locator('#subject-wrap > *');
  switch (step.action) {
    case 'focus':
      await page.evaluate(() => window.tauReference.focus());
      break;
    case 'blur':
      await page.evaluate(() => window.tauReference.blur());
      break;
    case 'click':
      await pointer(page, control);
      break;
    case 'hover':
      await pointer(page, control, false);
      break;
    case 'mouseDown': {
      await pointer(page, control, false);
      await page.mouse.down();
      break;
    }
    case 'mouseUp':
      await page.mouse.up();
      break;
    case 'pointer':
      await page.mouse.move(step.x, step.y);
      if (step.phase === 'down') await page.mouse.down();
      if (step.phase === 'up') await page.mouse.up();
      break;
    case 'key':
      await page.keyboard.press(step.key);
      break;
    case 'type':
      await page.keyboard.type(step.text);
      break;
    case 'select':
      await page.evaluate(
        ({ start, end }) => window.tauReference.select(start, end),
        step,
      );
      break;
    case 'props':
      await page.evaluate((p) => window.tauReference.patch(p), step.props);
      break;
    case 'checkpoint':
      break;
  }
}
async function visibleCaretFrame(page: Page): Promise<Buffer> {
  const observation = await page.evaluate(() => window.tauReference.observe());
  if (
    !observation.focused ||
    observation.disabled ||
    observation.readonly ||
    !observation.selection ||
    observation.selection[0] !== observation.selection[1]
  )
    throw new Error(
      'visible caret requires a focused editable collapsed selection',
    );
  await page.locator('#subject-wrap > *').evaluate((el) => {
    (el as HTMLElement).style.setProperty('caret-animation', 'manual');
  });
  const hidden = PNG.sync.read(
    await page.screenshot({ animations: 'disabled', caret: 'hide' }),
  );
  const bounds = await page.locator('#subject-wrap > *').boundingBox();
  const scale = await page.evaluate(() => window.devicePixelRatio);
  if (!bounds) throw new Error('missing caret subject');
  // Capture the actual browser caret, including engines without caret-animation support.
  for (let attempt = 0; attempt < 12; attempt++) {
    const bytes = await page.screenshot({
      animations: 'disabled',
      caret: 'initial',
    });
    const visible = PNG.sync.read(bytes);
    let changed = 0,
      minX = hidden.width,
      maxX = -1,
      minY = hidden.height,
      maxY = -1;
    for (let i = 0; i < hidden.data.length; i += 4) {
      if (
        hidden.data[i] !== visible.data[i] ||
        hidden.data[i + 1] !== visible.data[i + 1] ||
        hidden.data[i + 2] !== visible.data[i + 2]
      ) {
        changed++;
        const x = (i / 4) % hidden.width,
          y = Math.floor(i / 4 / hidden.width);
        minX = Math.min(minX, x);
        maxX = Math.max(maxX, x);
        minY = Math.min(minY, y);
        maxY = Math.max(maxY, y);
      }
    }
    if (changed > 0) {
      if (
        maxX - minX + 1 > 3 * scale ||
        maxY - minY < 4 * scale ||
        minX < bounds.x * scale ||
        maxX >= (bounds.x + bounds.width) * scale ||
        minY < bounds.y * scale ||
        maxY >= (bounds.y + bounds.height) * scale
      )
        throw new Error(
          'visible caret capture changed pixels outside a narrow in-control stroke',
        );
      if (
        JSON.stringify(observation) !==
        JSON.stringify(await page.evaluate(() => window.tauReference.observe()))
      )
        throw new Error('caret capture changed editor state');
      return bytes;
    }
    await page.waitForTimeout(100);
  }
  throw new Error(
    'browser caret did not become visible; no synthetic cursor fallback',
  );
}

async function runScenario(
  page: Page,
  scenario: Scenario,
  suite: Suite,
  out: string,
): Promise<Capture[]> {
  await page.mouse.up();
  // Reset keyboard/pointer modality as well as the DOM node and its undo history.
  await page.mouse.click(620, 290);
  await page.evaluate(
    ({ component, props, theme }) =>
      window.tauReference.reset(component, props, theme),
    { ...scenario, theme: suite.theme },
  );
  await page.mouse.move(620, 290);
  const results: Capture[] = [];
  for (const step of scenario.steps) {
    if (step.action !== 'checkpoint') {
      await action(page, step);
      continue;
    }
    await page.evaluate(() => document.fonts.ready);
    const bounds = await page.locator('#subject-wrap > *').boundingBox();
    if (!bounds)
      throw new Error(`${scenario.id}/${step.name}: missing subject bounds`);
    const screenshot = `${scenario.id}--${step.name}.png`;
    if (step.caret === 'visible')
      await writeFile(resolve(out, screenshot), await visibleCaretFrame(page));
    else
      await page.screenshot({
        path: resolve(out, screenshot),
        animations: 'disabled',
        caret: 'hide',
      });
    results.push({
      scenario: scenario.id,
      checkpoint: step.name,
      observation: await page.evaluate(() => window.tauReference.observe()),
      bounds,
      screenshot,
      caret: step.caret ?? 'hidden',
    });
  }
  return results;
}
export async function runBrowser(
  suite: Suite,
  out: string,
  browserName: 'chromium' | 'webkit',
  afterLoad?: (page: Page) => Promise<void>,
  options: { deferStateAssertions?: boolean } = {},
): Promise<Capture[]> {
  validateSuite(suite);
  await mkdir(out, { recursive: true });
  for (const name of await readdir(out)) {
    if (
      name === 'failure.json' ||
      name === 'report.json' ||
      name === 'observations.json' ||
      name.endsWith('.png')
    )
      await rm(resolve(out, name));
  }
  // Vite treats port 0 as its default port; reserve an OS-assigned port and require it.
  const port = await new Promise<number>((ok, fail) => {
    const socket = createTcpServer();
    socket.once('error', fail);
    socket.listen(0, '127.0.0.1', () => {
      const address = socket.address();
      if (!address || typeof address === 'string')
        return fail(new Error('no TCP port'));
      socket.close(() => ok(address.port));
    });
  });
  const server = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../..', import.meta.url)),
    cacheDir: resolve(import.meta.dir, '../.cache/vite'),
    plugins: [vue()],
    server: { host: '127.0.0.1', port, strictPort: true },
  });
  let browser: Awaited<ReturnType<BrowserType['launch']>> | undefined;
  try {
    await server.listen();
    const address = server.httpServer?.address();
    if (!address || typeof address === 'string')
      throw new Error('Vite did not bind a TCP port');
    browser = await (browserName === 'chromium' ? chromium : webkit).launch();
    const context = await browser.newContext({
      viewport: suite.viewport,
      deviceScaleFactor: suite.scale,
      colorScheme: suite.theme,
      reducedMotion: 'reduce',
    });
    const blockedRequests: string[] = [];
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      if (
        url.protocol === 'http:' &&
        url.hostname === '127.0.0.1' &&
        url.port === String(address.port)
      )
        return route.continue();
      blockedRequests.push(`${url.protocol}//${url.host}`);
      return route.abort('blockedbyclient');
    });
    await context.routeWebSocket('**/*', (socket) => {
      const url = new URL(socket.url());
      if (
        url.protocol === 'ws:' &&
        url.hostname === '127.0.0.1' &&
        url.port === String(address.port)
      ) {
        socket.connectToServer();
      } else {
        blockedRequests.push(`${url.protocol}//${url.host}`);
        socket.close({
          code: 1008,
          reason: 'External connections are blocked',
        });
      }
    });
    const page = await context.newPage();
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`));
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(`console: ${message.text()}`);
    });
    const captures: Capture[] = [];
    try {
      await page.goto(
        `http://127.0.0.1:${address.port}/gpui/reference/index.html`,
      );
      await page.evaluate(async () => {
        await document.fonts.ready;
        if (
          !document.fonts.check('13px "Inter Variable"') ||
          ![...document.fonts].some(
            (font) =>
              font.family === 'Inter Variable' && font.status === 'loaded',
          )
        )
          throw new Error('Inter Variable font not loaded');
      });
      await afterLoad?.(page);
      for (const scenario of suite.scenarios) {
        captures.push(...(await runScenario(page, scenario, suite, out)));
      }
      assertCaptures(suite, captures, 'browser');
      const failures = suite.scenarios.flatMap((scenario) =>
        scenario.steps
          .filter((step) => step.action === 'checkpoint')
          .flatMap((step) => {
            if (step.action !== 'checkpoint') return [];
            const id = `${scenario.id}/${step.name}`;
            const capture = captures.find(
              (c) => `${c.scenario}/${c.checkpoint}` === id,
            )!;
            return stateDifferences(step.expected, capture.observation).map(
              (difference) => `${id}: ${difference}`,
            );
          }),
      );
      if (errors.length) failures.push(...errors);
      if (blockedRequests.length)
        failures.push(
          `blocked external requests: ${[...new Set(blockedRequests)].join(', ')}`,
        );
      await writeFile(
        resolve(out, 'observations.json'),
        JSON.stringify(captures, null, 2),
      );
      await writeFile(
        resolve(out, 'report.json'),
        JSON.stringify({ failures, checkpoints: captures.length }, null, 2),
      );
      if (
        errors.length ||
        blockedRequests.length ||
        (failures.length && !options.deferStateAssertions)
      )
        throw new Error(
          `Vue reference: ${failures.length} failures (report: ${resolve(out, 'report.json')})\n${failures.join('\n')}`,
        );
      return captures;
    } catch (error) {
      await writeFile(
        resolve(out, 'observations.json'),
        JSON.stringify(captures, null, 2),
      );
      await writeFile(
        resolve(out, 'failure.json'),
        JSON.stringify(
          {
            error: String(error),
            errors,
            blockedRequests,
            checkpoints: captures.length,
          },
          null,
          2,
        ),
      );
      throw error;
    }
  } finally {
    await browser?.close();
    await server.close();
  }
}
