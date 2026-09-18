import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript';

declare global {
  interface Window {
    __TAU_CLIPBOARD_WRITES__?: string[];
    __TAU_OPENER_CALLS__?: string[];
    __TAU_VIEWER_ESCAPE_HANDLER_CALLS__?: number;
    __TAURI_INTERNALS__?: {
      transformCallback: (callback: unknown) => unknown;
      invoke: (
        command: string,
        payload?: { text?: string },
      ) => Promise<unknown>;
    };
  }
}

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('5000 messages');
  await expect(page.locator('[data-index="4999"]')).toBeVisible();
});

test('copies transcript URLs and local paths from context menus', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    const openerCalls: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAU_OPENER_CALLS__ = openerCalls;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
        }
        if (command.startsWith('plugin:opener|')) openerCalls.push(command);
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(fixtureUrl);

  const message = page.locator('[data-message-id="fixture-markdown-showcase"]');
  const url = message.getByRole('link', { name: 'Tau docs' });
  const path = message.locator(
    '[data-tau-path="src/components/TranscriptView.vue"]',
  );

  await url.click({ button: 'right' });
  await expect(page.getByRole('menuitem')).toHaveText(['Copy URL']);
  await page.getByRole('menuitem', { name: 'Copy URL' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['https://example.com/tau/docs']);

  // The context menu augments rather than replaces the existing activation.
  await url.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_OPENER_CALLS__))
    .toEqual(['plugin:opener|open_url']);

  await path.click({ button: 'right' });
  await expect(page.getByRole('menuitem')).toHaveText([
    'Copy Path',
    'Copy Full Path',
  ]);
  await page.getByRole('menuitem', { name: 'Copy Path', exact: true }).click();
  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Full Path' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      'https://example.com/tau/docs',
      'src/components/TranscriptView.vue',
      '/Users/someone/code/tau/src/components/TranscriptView.vue',
    ]);

  await path.click();
  await path.press('Enter');
  await expect
    .poll(() => page.evaluate(() => window.__TAU_OPENER_CALLS__))
    .toEqual([
      'plugin:opener|open_url',
      'plugin:opener|open_path',
      'plugin:opener|open_path',
    ]);
});

test('previews remote files and keeps explicit copy actions', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    const previewCalls: Array<{ command: string; payload?: unknown }> = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    Object.assign(window, { __TAU_PREVIEW_CALLS__: previewCalls });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
          return Promise.resolve(null);
        }
        if (command === 'remote_preview_available')
          return Promise.resolve(true);
        if (command === 'prepare_remote_path') {
          previewCalls.push({ command, payload });
          return Promise.resolve({ kind: 'file', token: 'fixture-token' });
        }
        if (command === 'show_remote_preview') {
          previewCalls.push({ command, payload });
          return Promise.resolve(null);
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const path = page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' });

  await expect(path).toBeVisible();
  const pathBox = await path.boundingBox();
  if (!pathBox) throw new Error('Remote path has no bounds');
  await page.mouse.move(pathBox.x + 2, pathBox.y + pathBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    pathBox.x + pathBox.width - 2,
    pathBox.y + pathBox.height / 2,
    { steps: 4 },
  );
  await page.mouse.up();
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toEqual([]);
  await page.evaluate(() => window.getSelection()?.removeAllRanges());

  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Path', exact: true }).click();
  const feedback = page.locator('.path-feedback', {
    hasText: 'Path Copied',
  });
  await expect(feedback).toBeVisible();
  await expect
    .poll(async () => {
      const box = await feedback.boundingBox();
      const viewport = page.viewportSize();
      return Boolean(
        box &&
        viewport &&
        box.x >= 0 &&
        box.y >= 0 &&
        box.x + box.width <= viewport.width &&
        box.y + box.height <= viewport.height,
      );
    })
    .toBe(true);
  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Full Path' }).click();
  await path.focus();
  await path.press('Space');

  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['src/remote.ts', '/home/agent/rhinestone/src/remote.ts']);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      {
        command: 'prepare_remote_path',
        payload: expect.objectContaining({
          projectPath: 'ssh:fixture-project',
          path: 'src/remote.ts',
        }),
      },
      {
        command: 'show_remote_preview',
        payload: expect.objectContaining({ token: 'fixture-token' }),
      },
    ]);
});

test('keeps duplicate remote activation attached to the original preview', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const calls: string[] = [];
    let resolvePreparation: ((value: unknown) => void) | undefined;
    Object.assign(window, {
      __TAU_PREVIEW_CALLS__: calls,
      __TAU_RESOLVE_PREVIEW__: (value: unknown) => resolvePreparation?.(value),
    });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> => {
        if (command === 'remote_preview_available')
          return Promise.resolve(true);
        if (command === 'prepare_remote_path') {
          calls.push(command);
          return new Promise((resolve) => {
            resolvePreparation = resolve;
          });
        }
        if (command === 'show_remote_preview') calls.push(command);
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const path = page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' });
  await path.click();
  await expect(page.locator('.path-feedback')).toHaveText('Preparing preview…');
  await path.click();
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: string[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toEqual(['prepare_remote_path']);
  await page.evaluate(() =>
    (
      window as Window & {
        __TAU_RESOLVE_PREVIEW__?: (value: unknown) => void;
      }
    ).__TAU_RESOLVE_PREVIEW__?.({ kind: 'file', token: 'original-token' }),
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: string[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual(['prepare_remote_path', 'show_remote_preview']);
  await expect(page.locator('.path-feedback')).toHaveCount(0);
});

test('wraps and clamps reviewed remote preview failure copy', async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> => {
        if (command === 'remote_preview_available')
          return Promise.resolve(true);
        if (command === 'prepare_remote_path')
          return Promise.reject({ kind: 'not_found_or_unreadable' });
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const path = page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' });
  await path.evaluate((element) => {
    Object.assign((element as HTMLElement).style, {
      position: 'fixed',
      right: '0',
      bottom: '0',
      zIndex: '1000',
    });
  });
  await path.click();
  const feedback = page.locator('.path-feedback[role="status"]');
  await expect(feedback).toHaveText(
    'File could not be read. Check that it exists and you have access.',
  );
  await expect
    .poll(async () => {
      const box = await feedback.boundingBox();
      const viewport = page.viewportSize();
      return feedback.evaluate(
        (element, bounds) =>
          Boolean(
            bounds.box &&
            bounds.viewport &&
            bounds.box.x >= 8 &&
            bounds.box.y >= 8 &&
            bounds.box.x + bounds.box.width <= bounds.viewport.width - 8 &&
            bounds.box.y + bounds.box.height <= bounds.viewport.height - 8 &&
            element.scrollWidth <= element.clientWidth &&
            element.scrollHeight <= element.clientHeight,
          ),
        { box, viewport },
      );
    })
    .toBe(true);
});

test('decodes explicit file destinations and keeps invalid ones inert', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const previewCalls: Array<{ command: string; payload?: unknown }> = [];
    Object.assign(window, { __TAU_PREVIEW_CALLS__: previewCalls });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string, payload?: unknown): Promise<unknown> => {
        if (command === 'remote_preview_available')
          return Promise.resolve(true);
        if (command === 'prepare_remote_path') {
          previewCalls.push({ command, payload });
          return Promise.resolve({ kind: 'file', token: crypto.randomUUID() });
        }
        if (command === 'show_remote_preview') return Promise.resolve(null);
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  await page.getByRole('link', { name: 'Encoded file' }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      {
        command: 'prepare_remote_path',
        payload: expect.objectContaining({ path: 'docs/My File.md' }),
      },
    ]);

  const url = page.url();
  await page.getByRole('link', { name: 'Invalid file' }).click();
  expect(page.url()).toBe(url);
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toHaveLength(1);
});

test('renders a long transcript without pagination controls or an oversized DOM', async ({
  page,
}) => {
  await expect(page.getByText(/Load (earlier|newer) messages/)).toHaveCount(0);

  const renderedRows = await page.locator('.message').count();
  expect(renderedRows).toBeGreaterThan(0);
  expect(renderedRows).toBeLessThan(50);

  const transcript = page.getByLabel('Transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(page.locator('[data-index="0"]')).toBeVisible();
  expect(await page.locator('.message').count()).toBeLessThan(50);
});

test('shows skill use as a collapsed expandable row', async ({ page }) => {
  const skill = page.locator('[data-message-id="fixture-skill-4998"]');
  const header = skill.locator('.activity-header');

  await expect(skill.locator('.activity-name')).toHaveText('skill');
  await expect(skill.locator('.activity-argument')).toHaveText(
    'desktop-app-native-feel',
  );
  await expect(header).not.toContainText(
    'Focus on keyboard behavior and perceived performance',
  );
  await expect(header).toHaveAttribute('aria-expanded', 'false');
  await expect(skill.locator('.activity-details')).toHaveCount(0);

  await header.click();

  await expect(header).toHaveAttribute('aria-expanded', 'true');
  await expect(skill.locator('.activity-detail-label')).toHaveText([
    'Prompt',
    'Instructions',
  ]);
  await expect(skill.locator('.activity-details')).toContainText(
    'Focus on keyboard behavior and perceived performance',
  );
  await expect(skill.locator('.activity-details')).toContainText(
    'Inspect selection, scrolling, keyboard behavior',
  );
});

test('keeps a thought behind its label until it is opened', async ({
  page,
}) => {
  const thought = page.locator('[data-message-id="fixture-thinking-trace"]');
  const header = thought.locator('.activity-header');

  await expect(thought.locator('.activity-name')).toHaveText('thinking');
  await expect(thought).not.toContainText('The adoption walk stops');
  await expect(header).toHaveAttribute('aria-expanded', 'false');

  await header.click();

  await expect(header).toHaveAttribute('aria-expanded', 'true');
  const prose = thought.locator('.activity-thinking-prose');
  await expect(thought.locator('.activity-details')).toContainText(
    'The adoption walk stops at the first local error row',
  );
  await expect(prose).toHaveCSS('color', 'rgb(110, 117, 127)');
  await expect(prose).toHaveCSS('font-size', '11px');
});

test('normalizes provider-formatted thinking sections', async ({ page }) => {
  const thought = page.locator('[data-message-id="fixture-thinking-sections"]');
  await thought.locator('.activity-header').click();

  const paragraphs = thought.locator('.activity-thinking-prose p');
  const emphasis = paragraphs.locator('strong');
  await expect(paragraphs).toHaveCount(2);
  await expect(emphasis).toHaveCount(2);
  await expect(paragraphs.first()).toHaveCSS('margin-top', '0px');
  await expect(paragraphs.first()).toHaveCSS('margin-bottom', '0px');
  await expect(paragraphs.last()).toHaveCSS('margin-top', '0px');
  await expect(paragraphs.last()).toHaveCSS('margin-bottom', '0px');
  await expect(emphasis.first()).toHaveCSS('font-weight', '400');
  await expect(emphasis.first()).toHaveCSS('color', 'rgb(110, 117, 127)');
  await expect(emphasis.first()).toHaveCSS('font-size', '11px');
});

test('marks only the calls that are running or failed', async ({ page }) => {
  const running = page.locator('[data-message-id="fixture-tool-running"]');
  const failed = page.locator('[data-message-id="fixture-tool-failed"]');
  const done = page.locator('[data-message-id="fixture-tool-done"]');

  const runningMark = running.locator('.activity-mark.running');
  const failedMark = failed.locator('svg.activity-mark.failed');
  await expect(runningMark).toHaveCount(1);
  await expect(failedMark).toHaveCount(1);
  await expect(done.locator('.activity-mark')).toHaveCount(0);
  await expect(runningMark).toHaveCSS('width', '11px');
  await expect(failedMark).toHaveCSS('width', '10px');
  await expect(failedMark).toHaveCSS('height', '10px');
  await expect(failedMark).toHaveCSS('font-size', '10px');

  // A row with nothing to report reserves nothing, and differently sized marks
  // still end in one column.
  const runningBox = await runningMark.boundingBox();
  const failedBox = await failedMark.boundingBox();
  expect(runningBox).not.toBeNull();
  expect(failedBox).not.toBeNull();
  const runningEnd = (runningBox?.x ?? 0) + (runningBox?.width ?? 0);
  const failedEnd = (failedBox?.x ?? 0) + (failedBox?.width ?? 0);
  expect(Math.abs(runningEnd - failedEnd)).toBeLessThan(1);
});

test('names the failed result as an error when the row is opened', async ({
  page,
}) => {
  const failed = page.locator('[data-message-id="fixture-tool-failed"]');

  await failed.locator('.activity-header').click();

  await expect(failed.locator('.activity-detail-label')).toHaveText([
    'Arguments',
    'Error',
  ]);
  await expect(failed.locator('.activity-detail-body.failed')).toContainText(
    'no match found for the replacement anchor',
  );
});

test('marks a task list with its checkbox alone, at full strength', async ({
  page,
}) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const task = showcase
    .locator('li', { has: page.locator('input[type="checkbox"]') })
    .first();
  const plain = showcase.locator('li').first();

  // The checkbox is the marker: a disc beside it would be a second one.
  await expect(task).toHaveCSS('list-style-type', 'none');
  await expect(plain).toHaveCSS('list-style-type', 'disc');

  // Task boxes are disabled inputs, which the app fades everywhere else.
  await expect(task.locator('input[type="checkbox"]')).toHaveCSS(
    'opacity',
    '1',
  );

  // The box hangs into the marker column, leaving both items on one text column.
  const [checkbox, taskItem, plainItem] = await Promise.all([
    task.locator('input[type="checkbox"]').boundingBox(),
    task.boundingBox(),
    plain.boundingBox(),
  ]);
  expect(taskItem?.x).toBeCloseTo(plainItem?.x ?? 0, 0);
  expect(checkbox?.x ?? 0).toBeLessThan(taskItem?.x ?? 0);
});

test('aligns table columns the way the markdown asked', async ({ page }) => {
  const headers = page
    .locator('[data-message-id="fixture-markdown-showcase"] table')
    .first()
    .locator('thead th');

  // Sanitizing keeps marked's `align` attribute, and the stylesheet defers to
  // it: a left default that overrode it would make `|--:|` do nothing.
  // WebKit and Chromium report an `align` attribute as `-webkit-<side>`.
  await expect(headers.nth(0)).toHaveCSS('text-align', 'left');
  await expect(headers.nth(1)).toHaveCSS('text-align', /^(?:-webkit-)?left$/);
  await expect(headers.nth(2)).toHaveCSS('text-align', /^(?:-webkit-)?center$/);
  await expect(headers.nth(3)).toHaveCSS('text-align', /^(?:-webkit-)?right$/);
});

test('omits a wholly empty table header but keeps partial headers and alignment', async ({
  page,
}) => {
  const tables = page.locator(
    '[data-message-id="fixture-markdown-showcase"] table',
  );
  const emptyHeader = tables.nth(2);
  const partialHeader = tables.nth(3);

  await expect(emptyHeader.locator('thead')).toHaveCount(0);
  await expect(emptyHeader.locator('tbody td').first()).toHaveCSS(
    'text-align',
    /^(?:-webkit-)?left$/,
  );
  await expect(emptyHeader.locator('tbody td').nth(1)).toHaveCSS(
    'text-align',
    /^(?:-webkit-)?right$/,
  );
  await expect(partialHeader.locator('thead th')).toHaveText(['', 'Kept']);
});

test('keeps a table header visible against the user bubble', async ({
  page,
}) => {
  const bubble = page.locator(
    '[data-message-id="fixture-markdown-showcase-user"] .user-bubble',
  );

  const [header, bubbleBackground] = await Promise.all([
    bubble.locator('thead th').first().evaluate(backgroundColor),
    bubble.evaluate(backgroundColor),
  ]);
  expect(header).not.toBe(bubbleBackground);
});

test('sizes user message surfaces to their content up to 90%', async ({
  page,
}) => {
  const shortMessage = page.locator('[data-message-id="fixture-short-user"]');
  const shortBubble = shortMessage.locator('.user-bubble');
  const longMessage = page.locator(
    '[data-message-id="fixture-markdown-showcase-user"]',
  );
  const longBubble = longMessage.locator('.user-bubble');

  await expect(shortBubble).toHaveCSS('background-color', 'rgb(235, 238, 240)');

  const [shortMessageBox, shortBubbleBox, longMessageBox, longBubbleBox] =
    await Promise.all([
      shortMessage.boundingBox(),
      shortBubble.boundingBox(),
      longMessage.boundingBox(),
      longBubble.boundingBox(),
    ]);
  expect(shortBubbleBox?.width ?? 0).toBeLessThan(
    (shortMessageBox?.width ?? 0) * 0.9,
  );
  expect(longBubbleBox?.width ?? 0).toBeCloseTo(
    (longMessageBox?.width ?? 0) * 0.9,
    0,
  );
});

test('keeps notices compact while preserving the restored type scale', async ({
  page,
}) => {
  await expect(page.locator('html')).toHaveCSS('font-size', '13px');

  for (const index of [4990]) {
    const notice = page.locator(`[data-message-id="fixture-notice-${index}"]`);
    await expect(notice.locator('.notice-label')).toHaveCSS('font-size', '9px');
    await expect(notice.locator('.notice-message')).toHaveCSS(
      'font-size',
      '12px',
    );
    await expect(notice.locator('.notice-message')).toHaveCSS(
      'line-height',
      '18px',
    );
  }
});

test('holds a wide image inside the message column', async ({ page }) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const image = showcase.locator('img');

  // The fixture image is far wider than any message column, and an image left
  // at its natural size would widen the transcript rather than scale into it.
  await expect(image).toHaveJSProperty('naturalWidth', 1_200);
  const [drawn, column] = await Promise.all([
    image.boundingBox(),
    showcase.locator('.markdown').first().boundingBox(),
  ]);
  expect(drawn?.width ?? 0).toBeLessThanOrEqual(column?.width ?? 0);
  expect(drawn?.width ?? 0).toBeGreaterThan(0);
});

test('keeps the reader at the end when a row grows after it was measured', async ({
  page,
}) => {
  const image = page
    .locator('[data-message-id="fixture-markdown-showcase"] img')
    .first();
  await expect(image).toHaveJSProperty('complete', true);

  // A row measured before its image decoded grows once it does, which moves
  // the end away from a reader who never scrolled: growth is not leaving.
  await expect
    .poll(() => page.getByLabel('Transcript').evaluate(distanceFromEnd))
    .toBeLessThan(2);
});

test('keeps a key and a rule through sanitizing', async ({ page }) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );

  // Inline HTML the agent writes for a shortcut, and a themed `hr`: both are
  // dropped by a sanitizer that does not allow them.
  await expect(showcase.locator('kbd').first()).toHaveText('Cmd');
  await expect(showcase.locator('hr')).toHaveCSS('border-top-width', '1px');
});

test('uses the real italic face and quieter emphasis weights', async ({
  page,
}) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );

  await expect(showcase.locator('em')).toHaveCSS('font-style', 'italic');
  await expect(showcase.locator('strong').first()).toHaveCSS(
    'font-weight',
    '600',
  );
  await expect(showcase.locator('h1')).toHaveCSS('font-weight', '650');

  const italicLoaded = await page.evaluate(async () => {
    await document.fonts.ready;
    let found = false;
    document.fonts.forEach((face) => {
      if (face.family.includes('Inter Variable') && face.style === 'italic') {
        found = true;
      }
    });
    return found;
  });
  expect(italicLoaded).toBe(true);
});

test('keeps prose narrower than wide blocks and tightens code leading', async ({
  page,
}) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const markdown = showcase.locator('.markdown').first();
  const prose = markdown.locator('> p').first();
  const block = markdown.locator('.code-block').first();

  const [markdownBox, proseBox, blockBox] = await Promise.all([
    markdown.boundingBox(),
    prose.boundingBox(),
    block.boundingBox(),
  ]);
  expect(proseBox?.width ?? 0).toBeLessThan(markdownBox?.width ?? 0);
  expect(blockBox?.width ?? 0).toBeCloseTo(markdownBox?.width ?? 0, 0);

  const leading = await block.locator('code').evaluate((code) => {
    const style = getComputedStyle(code);
    return (
      Number.parseFloat(style.lineHeight) / Number.parseFloat(style.fontSize)
    );
  });
  expect(leading).toBeCloseTo(1.5, 2);
});

test('highlights a fenced block in both schemes at once', async ({ page }) => {
  const block = page
    .locator('[data-message-id="fixture-markdown-showcase"] .code-block')
    .first();

  await expect(block.locator('pre')).toHaveClass(/shiki/);

  // A comment is a scope of its own, and the one place the palette leans on
  // italics: a grammar that failed to load would leave the block one colour.
  const comment = block.locator('span', { hasText: 'A comment' }).last();
  await expect(comment).toHaveCSS('font-style', 'italic');

  const light = await block.evaluate(tokenColors);
  expect(light.length).toBeGreaterThan(3);

  // Both schemes ride along on every token, so the dark palette is a repaint
  // rather than another pass over the markdown.
  await page.emulateMedia({ colorScheme: 'dark' });
  const dark = await block.evaluate(tokenColors);
  expect(dark.length).toBeGreaterThan(3);
  expect(dark).not.toEqual(light);
  await page.emulateMedia({ colorScheme: 'light' });
});

test('names a block the language it was fenced with', async ({ page }) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const block = showcase.locator('.code-block').first();

  // The tag as written, not the grammar it resolves to: `console` is the label
  // even though a shell grammar draws it.
  await expect(block).toHaveAttribute('data-tau-lang', 'ts');
  await expect(
    showcase.locator('.code-block[data-tau-lang="console"]'),
  ).toHaveCount(1);
  // A fence that named no language has nothing to label.
  await expect(
    showcase.locator('.code-block:not([data-tau-lang])'),
  ).toHaveCount(1);

  // The label keeps the copy button's terms: shown to a reader who is pointing
  // at the block, and out of the way otherwise.
  await expect.poll(() => block.evaluate(labelOpacity)).toBe('0');
  await block.hover();
  await expect.poll(() => block.evaluate(labelOpacity)).not.toBe('0');
  expect(await block.evaluate(labelContent)).toContain('ts');
  expect(
    await block.evaluate(
      (element) => getComputedStyle(element, '::before').fontSize,
    ),
  ).toBe('10px');
});

test('draws a fenced diagram in the scheme around it', async ({ page }) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const diagram = showcase.locator('.diagram svg').first();

  // The renderer is loaded only once a transcript holds a diagram.
  await expect(showcase.locator('.diagram')).toHaveCount(2, {
    timeout: 20_000,
  });
  await expect(
    diagram.locator('text', { hasText: 'Session live?' }),
  ).toHaveCount(1);

  // A fence the parser cannot read, and one that has not reached its closing
  // fence, both stay the source the reader was sent.
  await expect(
    showcase.locator('.code-block[data-tau-lang="mermaid"]'),
  ).toHaveCount(2);

  // Ruled like the block its source would have been, but not filled like one:
  // the page a diagram is drawn on is the one the message is on.
  const block = showcase.locator('.diagram').first();
  await expect(block).toHaveCSS('border-top-width', '1px');
  await expect(block).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');

  // Held to the message column rather than widening it.
  const width = await diagram.evaluate(
    (svg) =>
      svg.getBoundingClientRect().width /
      (svg.closest('.markdown')?.getBoundingClientRect().width ?? 1),
  );
  expect(width).toBeLessThanOrEqual(1);

  // Every colour is a reference to a token the app draws itself with, so the
  // other scheme is a repaint rather than another diagram.
  const light = await diagram.evaluate(diagramColors);
  await page.emulateMedia({ colorScheme: 'dark' });
  expect(await diagram.evaluate(diagramColors)).not.toEqual(light);
  await page.emulateMedia({ colorScheme: 'light' });
});

test('expands a drawn diagram into a fullscreen pan-and-zoom viewer', async ({
  page,
}) => {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  const figure = showcase.locator('.diagram').first();
  const button = figure.locator('.diagram-expand');

  await expect(showcase.locator('.diagram')).toHaveCount(2, {
    timeout: 20_000,
  });
  // Only drawn diagrams carry the button; the fences that stayed source do not.
  await expect(showcase.locator('.diagram-expand')).toHaveCount(2);

  // The button keeps the copy button's terms: revealed by pointing at the figure.
  await expect
    .poll(() => button.evaluate((element) => getComputedStyle(element).opacity))
    .toBe('0');
  await figure.hover();
  await expect
    .poll(() => button.evaluate((element) => getComputedStyle(element).opacity))
    .not.toBe('0');

  await button.click();
  const viewer = page.locator('.diagram-viewer');
  await expect(viewer).toBeVisible();
  await expect(viewer).toHaveRole('dialog');
  await expect(viewer).toHaveAccessibleName('Diagram');

  // The same drawing opens fitted from its natural dimensions. The SVG itself
  // owns the fullscreen viewport: no transformed ancestor can turn it into a
  // bitmap layer for the compositor to enlarge.
  const drawing = viewer.locator('.viewer-drawing');
  const vector = drawing.locator(':scope > svg');
  await expect(
    vector.locator('text', { hasText: 'Session live?' }),
  ).toHaveCount(1);
  await expect(vector).toHaveAttribute('width', '100%');
  await expect(vector).toHaveAttribute('height', '100%');
  await expect(vector).toHaveAttribute('preserveAspectRatio', 'none');
  const compositingStyles = await vector.evaluate((element) => {
    const styles: Array<{ transform: string; willChange: string }> = [];
    for (
      let current: Element | null = element;
      current;
      current = current.parentElement
    ) {
      const style = getComputedStyle(current);
      styles.push({ transform: style.transform, willChange: style.willChange });
    }
    return styles;
  });
  expect(compositingStyles.every(({ transform }) => transform === 'none')).toBe(
    true,
  );
  expect(
    compositingStyles.every(({ willChange }) => willChange === 'auto'),
  ).toBe(true);

  const naturalSize = await figure.evaluate((element) => {
    const svg = element.querySelector('svg');
    return {
      width: Number.parseFloat(svg?.getAttribute('width') ?? ''),
      height: Number.parseFloat(svg?.getAttribute('height') ?? ''),
    };
  });
  const canvas = viewer.locator('.viewer-canvas');
  const canvasBox = (await canvas.boundingBox())!;
  const viewBox = (): Promise<[number, number, number, number]> =>
    vector.evaluate((element) => {
      const box = (element as SVGSVGElement).viewBox.baseVal;
      return [box.x, box.y, box.width, box.height];
    });
  const scaleOf = async (): Promise<number> => {
    const box = await viewBox();
    return canvasBox.width / box[2];
  };
  const fitted = await scaleOf();
  expect(fitted).toBeCloseTo(
    Math.min(
      (canvasBox.width - 96) / naturalSize.width,
      (canvasBox.height - 96) / naturalSize.height,
      1.5,
    ),
    3,
  );

  // A pinch arrives as a ctrl-wheel, zooms around its pointer, and causes a
  // new SVG viewBox layout rather than a CSS scale. A plain wheel pans it.
  const zoomPoint = { x: 400, y: 300 };
  const beforeZoom = await viewBox();
  const worldAtPointer = {
    x: beforeZoom[0] + (zoomPoint.x / canvasBox.width) * beforeZoom[2],
    y: beforeZoom[1] + (zoomPoint.y / canvasBox.height) * beforeZoom[3],
  };
  await canvas.dispatchEvent('wheel', {
    deltaY: -200,
    ctrlKey: true,
    clientX: zoomPoint.x,
    clientY: zoomPoint.y,
  });
  await expect.poll(scaleOf).toBeGreaterThan(fitted);
  const afterZoom = await viewBox();
  expect(
    afterZoom[0] + (zoomPoint.x / canvasBox.width) * afterZoom[2],
  ).toBeCloseTo(worldAtPointer.x, 3);
  expect(
    afterZoom[1] + (zoomPoint.y / canvasBox.height) * afterZoom[3],
  ).toBeCloseTo(worldAtPointer.y, 3);

  const beforePan = await viewBox();
  await canvas.dispatchEvent('wheel', { deltaX: 40, deltaY: 60 });
  await expect.poll(viewBox).not.toEqual(beforePan);
  expect(await scaleOf()).toBeGreaterThan(fitted);

  // Dragging pans without closing: the viewer only leaves on a clean click.
  const box = (await canvas.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2);
  await page.mouse.up();
  await expect(viewer).toBeVisible();

  // The viewer wears no chrome, so a plain click is the exit, and focus
  // returns to the button that opened it.
  await canvas.click({ position: { x: 40, y: 40 } });
  await expect(viewer).toHaveCount(0);
  await expect(button).toBeFocused();

  // The whole loop works from the keyboard: Enter reopens, Escape closes.
  await page.keyboard.press('Enter');
  await expect(viewer).toBeVisible();

  // The viewer owns Escape before document's bubble phase, so an app-level
  // handler must not see the key that closes it.
  await page.evaluate(() => {
    window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__ = 0;
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__! += 1;
      }
    });
  });
  await page.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await expect(button).toBeFocused();
  expect(
    await page.evaluate(() => window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__),
  ).toBe(0);
});

test('copies a code block from a button the block reveals on hover', async ({
  page,
}) => {
  // The clipboard is a native command, which a browser test has to stand in
  // for; the writes it records are what the assertion reads.
  await page.addInitScript(() => {
    const writes: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(fixtureUrl);

  const message = page.locator('[data-message-id="fixture-markdown-showcase"]');
  const block = message.locator('.code-block').first();
  const copy = block.getByRole('button', { name: 'Copy Code' });
  const box = (await block.boundingBox()) ?? {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  };

  await expect(block.locator('pre')).toContainText(
    'export async function load',
  );
  await expect(copy).toHaveCSS('opacity', '0');

  await block.hover();
  await expect(copy).toHaveCSS('opacity', '0.45');

  // Pointing at the button itself is what takes it to full strength.
  await copy.hover();
  await expect(copy).toHaveCSS('opacity', '1');

  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      "export async function load(id: string): Promise<Session | null> {\n  // A comment, italic in both schemes\n  const session = await invoke<Session>('load_session', { id });\n  return session ?? null;\n}\n",
    ]);
  await expect(copy).toHaveAttribute('data-copied', 'true');

  // The acknowledgement belongs to the hovered block: leaving hides the button
  // even while it is still showing, and it is temporary in any case.
  await page.mouse.move(box.x + box.width / 2, box.y - 40);
  await expect(copy).toHaveCSS('opacity', '0');
  await expect(copy).not.toHaveAttribute('data-copied', 'true', {
    timeout: 3_000,
  });
});

test('leaves remote paths in fenced transcript markdown as code', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const message = page.locator('[data-message-id="fixture-remote-paths"]');
  const workspace = message.getByRole('button', {
    name: 'Preview path /home/agent/rhinestone/workspace',
  });
  const block = message.locator('.code-block');
  const copy = block.getByRole('button', { name: 'Copy Code' });

  // Inline paths retain their remote-copy interaction, but full-width code is
  // literal content and must not become a row of independent path controls.
  await expect(workspace).toBeVisible();
  await expect(block.getByRole('button', { name: /Preview path/ })).toHaveCount(
    0,
  );
  await expect(block).toContainText('/home/agent/rhinestone/orchestrator');
  await expect(block).toContainText(
    '/home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md',
  );

  await workspace.focus();
  await workspace.press('Space');
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['/home/agent/rhinestone/workspace']);
  await expect(page.locator('.path-feedback')).toHaveText('Path Copied');

  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      '/home/agent/rhinestone/workspace',
      'Repo /home/agent/rhinestone/orchestrator\nPlan /home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md\n/usage\n/ expanded\n</pre>\n',
    ]);
});

test('opens a tool call in place and keeps it open across virtualization', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = element.scrollHeight * 0.45;
  });
  await page.waitForTimeout(150);

  const id = await page
    .locator('[data-message-id^="fixture-tool-"]')
    .first()
    .evaluate((element) => (element as HTMLElement).dataset.messageId ?? '');
  const call = page.locator(`[data-message-id="${id}"]`);
  const header = call.locator('.activity-header');
  await expect(header).toHaveAttribute('aria-expanded', 'false');
  await expect(call.locator('.activity-details')).toHaveCount(0);

  await header.click();
  await expect(call.locator('.activity-detail-label').first()).toHaveText(
    'Arguments',
  );
  await expect(header).toHaveAttribute('aria-expanded', 'true');

  // The row is unmounted while it is out of view, so the open state cannot live
  // in the row itself.
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(page.locator('[data-index="0"]')).toBeVisible();
  await transcript.evaluate((element) => {
    element.scrollTop = element.scrollHeight * 0.45;
  });
  await page.waitForTimeout(150);

  await expect(header).toHaveAttribute('aria-expanded', 'true');
});

test('does not move a reader in history when output changes', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = element.scrollHeight * 0.45;
  });
  await page.waitForTimeout(150);

  const before = await transcript.evaluate(anchorSnapshot);
  expect(before).not.toBeNull();

  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage();
  });
  await expect(page.getByTestId('fixture-count')).toHaveText('5001 messages');
  await page.waitForTimeout(150);

  const afterAppend = await transcript.evaluate(anchorSnapshot);
  expect(afterAppend?.id).toBe(before?.id);
  expect(
    Math.abs((afterAppend?.offset ?? 0) - (before?.offset ?? 0)),
  ).toBeLessThan(2);

  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.replaceLatestMessage();
  });
  await page.waitForTimeout(150);

  const afterReplacement = await transcript.evaluate(anchorSnapshot);
  expect(afterReplacement?.id).toBe(before?.id);
  expect(
    Math.abs((afterReplacement?.offset ?? 0) - (before?.offset ?? 0)),
  ).toBeLessThan(2);
});

test('keeps streaming output pinned to the end', async ({ page }) => {
  const transcript = page.getByLabel('Transcript');
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
  await expect(page.locator('[data-index="4999"]')).toBeVisible();
});

test('follows a row appended while the working indicator is shown', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(true);
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('tool');
  });
  await expect(page.getByTestId('fixture-count')).toHaveText('5001 messages');

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
  await expect(page.locator('[data-index="5000"]')).toBeVisible();
});

test('follows the reply that replaces the working indicator', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(true);
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('tool');
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  // The indicator gives way to the reply it stood in for, leaving the row count
  // unchanged across the swap.
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('assistant');
  });
  await expect(page.getByTestId('fixture-count')).toHaveText('5002 messages');

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
  await expect(page.locator('[data-index="5001"]')).toBeVisible();
});

test('stops following output once the reader scrolls back', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await transcript.hover();
  await page.mouse.wheel(0, -400);
  await page.waitForTimeout(150);

  const before = await transcript.evaluate(anchorSnapshot);
  expect(before).not.toBeNull();

  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(true);
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('tool');
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('assistant');
    window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(false);
  });
  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );
  await page.waitForTimeout(150);

  const after = await transcript.evaluate(anchorSnapshot);
  expect(after?.id).toBe(before?.id);
  expect(Math.abs((after?.offset ?? 0) - (before?.offset ?? 0))).toBeLessThan(
    2,
  );
  expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);
});

test('follows output again once the reader returns to the end', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.hover();
  await page.mouse.wheel(0, -400);
  await page.waitForTimeout(150);
  expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);

  await page.mouse.wheel(0, 2000);
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
});

test('keeps the reader at the end when the viewport shrinks under them', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  // Standing in for everything that takes height from the transcript under a
  // reader sitting at the end: the composer growing a line, a status line, a
  // sidebar drag. Scroll position survives all of them, so the end walks away.
  await page.setViewportSize({ width: 1280, height: 500 });

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
});

test('leaves a reader in history where they are when the viewport shrinks', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.hover();
  await page.mouse.wheel(0, -400);
  await page.waitForTimeout(150);

  const before = await transcript.evaluate(anchorSnapshot);
  expect(before).not.toBeNull();

  await page.setViewportSize({ width: 1280, height: 500 });
  await page.waitForTimeout(150);

  const after = await transcript.evaluate(anchorSnapshot);
  expect(after?.id).toBe(before?.id);
  expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);
});

test('follows output again after the reader sends', async ({ page }) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.hover();
  await page.mouse.wheel(0, -400);
  await page.waitForTimeout(150);
  expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);

  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
  });
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
});

test('returns a session to the place in history it was left', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = element.scrollHeight * 0.45;
  });
  await page.waitForTimeout(150);

  const before = await transcript.evaluate(anchorSnapshot);
  expect(before).not.toBeNull();

  await switchSession(page, 'other');
  await switchSession(page, 'main');

  const after = await transcript.evaluate(anchorSnapshot);
  expect(after?.id).toBe(before?.id);
  expect(Math.abs((after?.offset ?? 0) - (before?.offset ?? 0))).toBeLessThan(
    2,
  );
});

test('returns a session left at the end to the end it has grown', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);

  await switchSession(page, 'other');

  // The session goes on streaming while it is off screen, so the end it was
  // left at is no longer the offset it was.
  await page.evaluate(() => {
    window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage();
  });
  await expect(page.getByTestId('fixture-count')).toHaveText('5001 messages');

  await switchSession(page, 'main');

  await expect.poll(() => transcript.evaluate(distanceFromEnd)).toBeLessThan(2);
  await expect(page.locator('[data-index="5000"]')).toBeVisible();
});

async function switchSession(page: Page, key: string): Promise<void> {
  await page.evaluate(
    (next) => window.__TAU_TRANSCRIPT_FIXTURE__?.switchSession(next),
    key,
  );
  await expect(page.getByTestId('fixture-session')).toHaveText(key);
  await page.waitForTimeout(150);
}

function backgroundColor(element: HTMLElement): string {
  return getComputedStyle(element).backgroundColor;
}

/** The distinct colours a highlighted block's tokens are drawn in. */
function tokenColors(element: HTMLElement): string[] {
  const spans = Array.from(element.querySelectorAll<HTMLElement>('pre span'));
  return [...new Set(spans.map((span) => getComputedStyle(span).color))];
}

/** The fills a drawn diagram is painted with, which its theme decides. */
function diagramColors(element: SVGElement): string[] {
  const painted = Array.from(
    element.querySelectorAll<SVGElement>('rect, text, polyline'),
  );
  return [
    ...new Set(painted.map((node) => getComputedStyle(node).fill)),
  ].sort();
}

function labelOpacity(element: HTMLElement): string {
  return getComputedStyle(element, '::before').opacity;
}

function labelContent(element: HTMLElement): string {
  return getComputedStyle(element, '::before').content;
}

function distanceFromEnd(element: HTMLElement): number {
  return element.scrollHeight - element.scrollTop - element.clientHeight;
}

function anchorSnapshot(element: HTMLElement): {
  id: string;
  offset: number;
} | null {
  const viewport = element.getBoundingClientRect();
  const rows = Array.from(
    element.querySelectorAll<HTMLElement>('[data-message-id]'),
  );
  const anchor = rows.find(
    (row) => row.getBoundingClientRect().bottom > viewport.top,
  );
  if (!anchor) return null;

  return {
    id: anchor.dataset.messageId ?? '',
    offset: anchor.getBoundingClientRect().top - viewport.top,
  };
}
