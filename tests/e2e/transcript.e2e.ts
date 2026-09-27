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

test('reads virtual history, holds its anchor through output and restores it after a session switch', async ({
  page,
}) => {
  await test.step('reads and expands a bounded virtual transcript without losing the history anchor', async () => {
    await test.step('keeps a large transcript bounded and preserves expanded rows across virtualization', async () => {
      const image = page
        .locator('[data-message-id="fixture-markdown-showcase"] img')
        .first();
      await expect(image).toHaveJSProperty('complete', true);
      await expect
        .poll(() => page.getByLabel('Transcript').evaluate(distanceFromEnd))
        .toBeLessThan(2);
      await expect(page.getByText(/Load (earlier|newer) messages/)).toHaveCount(
        0,
      );
      const renderedRows = await page.locator('.message').count();
      expect(renderedRows).toBeGreaterThan(0);
      expect(renderedRows).toBeLessThan(50);
      const transcript = page.getByLabel('Transcript');
      await transcript.evaluate((element) => {
        element.scrollTop = 0;
      });
      await expect(page.locator('[data-index="0"]')).toBeVisible();
      expect(await page.locator('.message').count()).toBeLessThan(50);
      await transcript.evaluate((element) => {
        element.scrollTop = element.scrollHeight;
      });
      await transcript.evaluate((element) => {
        element.scrollTop = element.scrollHeight * 0.45;
      });
      await page.waitForTimeout(150);
      const id = await page
        .locator('[data-message-id^="fixture-tool-"]')
        .first()
        .evaluate(
          (element) => (element as HTMLElement).dataset.messageId ?? '',
        );
      const call = page.locator(`[data-message-id="${id}"]`);
      const header = call.locator('.activity-header');
      await expect(header).toHaveAttribute('aria-expanded', 'false');
      await expect(call.locator('.activity-details')).toHaveCount(0);
      await header.click();
      await expect(call.locator('.activity-detail-label').first()).toHaveText(
        'Arguments',
      );
      await expect(header).toHaveAttribute('aria-expanded', 'true');
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
    await test.step('does not move a reader in history when output changes', async () => {
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
      await expect(page.getByTestId('fixture-count')).toHaveText(
        '5001 messages',
      );
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
  });

  await test.step('restores both history position and the growing end when returning to a session', async () => {
    const transcript = page.getByLabel('Transcript');
    await transcript.evaluate(
      (element) =>
        new Promise<void>((resolve) => {
          element.addEventListener('scrollend', () => resolve(), {
            once: true,
          });
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
    await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd());
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await switchSession(page, 'other');
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage();
    });
    await expect(page.getByTestId('fixture-count')).toHaveText('5002 messages');
    await switchSession(page, 'main');
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await expect(page.locator('[data-index="5001"]')).toBeVisible();
  });
});

test('follows live output at the end but respects scrolling back and viewport changes', async ({
  page,
}) => {
  await test.step('keeps streaming output pinned to the end', async () => {
    const transcript = page.getByLabel('Transcript');
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
    );
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await expect(page.locator('[data-index="4999"]')).toBeVisible();
  });

  await test.step('follows the working indicator, tool row, and replacement reply', async () => {
    const transcript = page.getByLabel('Transcript');
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
      window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(true);
    });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('tool');
    });
    await expect(page.getByTestId('fixture-count')).toHaveText('5001 messages');
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await expect(page.locator('[data-index="5000"]')).toBeVisible();
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.appendMessage('assistant');
    });
    await expect(page.getByTestId('fixture-count')).toHaveText('5002 messages');
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await expect(page.locator('[data-index="5001"]')).toBeVisible();
  });

  await test.step('stops following output once the reader scrolls back', async () => {
    const transcript = page.getByLabel('Transcript');
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
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

  await test.step('follows output again once the reader returns to the end', async () => {
    const transcript = page.getByLabel('Transcript');
    await transcript.hover();
    await page.mouse.wheel(0, -400);
    await page.waitForTimeout(150);
    expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);
    await page.mouse.wheel(0, 2000);
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
    );
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
  });

  await test.step('keeps the reader at the end when the viewport shrinks under them', async () => {
    const transcript = page.getByLabel('Transcript');
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.setViewportSize({ width: 1280, height: 500 });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
    );
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
  });

  await test.step('leaves a reader in history where they are when the viewport shrinks', async () => {
    await page.setViewportSize({ width: 1280, height: 720 });
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

  await test.step('follows output again after the reader sends', async () => {
    const transcript = page.getByLabel('Transcript');
    await transcript.hover();
    await page.mouse.wheel(0, -400);
    await page.waitForTimeout(150);
    expect(await transcript.evaluate(distanceFromEnd)).toBeGreaterThan(100);
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.scrollToEnd();
    });
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
    );
    await expect
      .poll(() => transcript.evaluate(distanceFromEnd))
      .toBeLessThan(2);
  });
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
