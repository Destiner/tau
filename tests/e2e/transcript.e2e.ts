import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript';

declare global {
  interface Window {
    __TAU_CLIPBOARD_WRITES__?: string[];
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
  await expect(thought.locator('.activity-details')).toContainText(
    'The adoption walk stops at the first local error row',
  );
});

test('marks only the calls that are running or failed', async ({ page }) => {
  const running = page.locator('[data-message-id="fixture-tool-running"]');
  const failed = page.locator('[data-message-id="fixture-tool-failed"]');
  const done = page.locator('[data-message-id="fixture-tool-done"]');

  await expect(running.locator('.activity-mark.running')).toHaveCount(1);
  await expect(failed.locator('.activity-mark.failed')).toHaveCount(1);
  await expect(done.locator('.activity-mark')).toHaveCount(0);

  // A row with nothing to report reserves nothing, and the rows that do report
  // still end in one column.
  const runningBox = await running.locator('.activity-mark').boundingBox();
  const failedBox = await failed.locator('.activity-mark').boundingBox();
  expect(runningBox).not.toBeNull();
  expect(failedBox).not.toBeNull();
  expect(Math.abs((runningBox?.x ?? 0) - (failedBox?.x ?? 0))).toBeLessThan(1);
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

  const message = page.locator('[data-message-id="fixture-assistant-4999"]');
  const block = message.locator('.code-block');
  const copy = block.getByRole('button', { name: 'Copy Code' });
  const box = (await block.boundingBox()) ?? {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  };

  await expect(block.locator('pre')).toContainText('const messageIndex = 4999');
  await expect(copy).toHaveCSS('opacity', '0');

  await block.hover();
  await expect(copy).toHaveCSS('opacity', '0.45');

  // Pointing at the button itself is what takes it to full strength.
  await copy.hover();
  await expect(copy).toHaveCSS('opacity', '1');

  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['const messageIndex = 4999;\nconsole.log({ messageIndex });\n']);
  await expect(copy).toHaveAttribute('data-copied', 'true');

  // The acknowledgement belongs to the hovered block: leaving hides the button
  // even while it is still showing, and it is temporary in any case.
  await page.mouse.move(box.x + box.width / 2, box.y - 40);
  await expect(copy).toHaveCSS('opacity', '0');
  await expect(copy).not.toHaveAttribute('data-copied', 'true', {
    timeout: 3_000,
  });
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

test('keeps frame delivery and mounted rows bounded during a full sweep', async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'Frame timing is asserted in Chromium');

  const transcript = page.getByLabel('Transcript');
  const metrics = await transcript.evaluate(async (element) => {
    const intervals: number[] = [];
    let maximumRows = 0;
    let longTaskDuration = 0;
    let previousFrame = performance.now();
    const startTop = element.scrollTop;
    const duration = 2_500;

    const observer =
      typeof PerformanceObserver !== 'undefined' &&
      PerformanceObserver.supportedEntryTypes.includes('longtask')
        ? new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
              longTaskDuration += entry.duration;
            }
          })
        : undefined;
    observer?.observe({ entryTypes: ['longtask'] });

    await new Promise<void>((resolve) => {
      const startedAt = performance.now();
      const frame = (now: number): void => {
        intervals.push(now - previousFrame);
        previousFrame = now;
        const progress = Math.min((now - startedAt) / duration, 1);
        const eased = progress * progress * (3 - 2 * progress);
        element.scrollTop = startTop * (1 - eased);
        maximumRows = Math.max(
          maximumRows,
          document.querySelectorAll('.message').length,
        );
        if (progress < 1) requestAnimationFrame(frame);
        else resolve();
      };
      requestAnimationFrame(frame);
    });

    observer?.disconnect();
    intervals.sort((left, right) => left - right);
    const percentile95 = intervals[Math.floor(intervals.length * 0.95)] ?? 0;
    const slowFrames = intervals.filter((interval) => interval > 50).length;

    return {
      frameCount: intervals.length,
      longTaskDuration,
      maximumRows,
      percentile95,
      slowFrameRatio: slowFrames / intervals.length,
    };
  });

  /*
   * Measured over a 2.5s sweep of 5000 rows: 110-117 frames, 24-25 mounted
   * rows, a 50ms 95th percentile, 3-8% slow frames, and 600-780ms of long
   * tasks. The frame pair is the guard that matches what a reader feels; the
   * long-task total is the same work seen in aggregate, so it is bounded
   * loosely enough to survive a busy machine and still catch a doubling.
   */
  expect(metrics.frameCount).toBeGreaterThan(90);
  expect(metrics.maximumRows).toBeLessThan(50);
  expect(metrics.percentile95).toBeLessThan(67);
  expect(metrics.slowFrameRatio).toBeLessThan(0.12);
  expect(metrics.longTaskDuration).toBeLessThan(1_000);
  await expect(page.locator('[data-index="0"]')).toBeVisible();
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
