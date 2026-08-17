import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript';

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

  const transcript = page.getByLabel('Tau transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(page.locator('[data-index="0"]')).toBeVisible();
  expect(await page.locator('.message').count()).toBeLessThan(50);
});

test('shows skill use as a collapsed expandable block', async ({ page }) => {
  const skill = page.locator('[data-message-id="fixture-skill-4998"]');
  const header = skill.locator('.skill-header');

  await expect(skill.locator('.skill-label')).toHaveText('skill');
  await expect(skill.locator('.skill-title')).toHaveText(
    'desktop-app-native-feel',
  );
  await expect(header).not.toContainText(
    'Focus on keyboard behavior and perceived performance',
  );
  await expect(header).toHaveAttribute('aria-expanded', 'false');
  await expect(skill.locator('.skill-details')).toHaveCount(0);

  await header.click();

  await expect(header).toHaveAttribute('aria-expanded', 'true');
  await expect(skill.locator('.skill-detail-label')).toHaveText([
    'Prompt',
    'Instructions',
  ]);
  await expect(skill.locator('.skill-details')).toContainText(
    'Focus on keyboard behavior and perceived performance',
  );
  await expect(skill.locator('.skill-details')).toContainText(
    'Inspect selection, scrolling, keyboard behavior',
  );
});

test('opens a tool call in place and keeps it open across virtualization', async ({
  page,
}) => {
  const transcript = page.getByLabel('Tau transcript');
  await transcript.evaluate((element) => {
    element.scrollTop = element.scrollHeight * 0.45;
  });
  await page.waitForTimeout(150);

  const id = await page
    .locator('.tool-call')
    .first()
    .evaluate(
      (element) =>
        (element.closest('[data-message-id]') as HTMLElement | null)?.dataset
          .messageId ?? '',
    );
  const call = page.locator(`[data-message-id="${id}"]`);
  const header = call.locator('.tool-header');
  await expect(header).toHaveAttribute('aria-expanded', 'false');
  await expect(call.locator('.tool-details')).toHaveCount(0);

  await header.click();
  await expect(call.locator('.tool-detail-label').first()).toHaveText(
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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
  const transcript = page.getByLabel('Tau transcript');
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

test('keeps frame delivery and mounted rows bounded during a full sweep', async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'Frame timing is asserted in Chromium');

  const transcript = page.getByLabel('Tau transcript');
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

  expect(metrics.frameCount).toBeGreaterThan(90);
  expect(metrics.maximumRows).toBeLessThan(50);
  expect(metrics.percentile95).toBeLessThan(67);
  expect(metrics.slowFrameRatio).toBeLessThan(0.08);
  expect(metrics.longTaskDuration).toBeLessThan(500);
  await expect(page.locator('[data-index="0"]')).toBeVisible();
});

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
