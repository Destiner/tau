import type { Page } from '@playwright/test';

import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript';

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('5000 messages');
  await expect(page.locator('[data-index="4999"]')).toBeVisible();
  await waitForShowcaseRendering(page);
  await expect
    .poll(() => page.getByLabel('Transcript').evaluate(distanceFromEnd))
    .toBeLessThan(2);
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
  await transcript.evaluate(
    (element) =>
      new Promise<void>((resolve) => {
        element.addEventListener('scrollend', () => resolve(), { once: true });
        element.scrollTop = element.scrollHeight * 0.45;
      }),
  );

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
