import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

/**
 * The transcript renders from an offset the virtualizer keeps for itself, and
 * only a scroll event refreshes it. These cover the moments where the two can
 * come apart — a transcript arriving whole, and one that shrank while it was
 * away — because what the reader sees when they do is rows drawn from further
 * down than they are looking at, with blank canvas above them.
 */

interface ViewportState {
  /** Blank canvas above the topmost rendered row, past the top padding. */
  gap: number;
  firstIndex: number;
  scrollTop: number;
}

async function viewportState(page: Page): Promise<ViewportState> {
  return await page.evaluate(() => {
    const element = document.querySelector(
      '[aria-label="Transcript"]',
    ) as HTMLElement | null;
    const rows = Array.from(document.querySelectorAll('[data-index]'));
    if (!element || rows.length === 0) {
      return { gap: Number.NaN, firstIndex: -1, scrollTop: -1 };
    }
    const indexes = rows.map((row) => Number(row.getAttribute('data-index')));
    const top = Math.min(...rows.map((row) => row.getBoundingClientRect().top));
    return {
      gap: Math.round(top - element.getBoundingClientRect().top - 28),
      firstIndex: Math.min(...indexes),
      scrollTop: Math.round(element.scrollTop),
    };
  });
}

test('hydrates a working transcript and reconciles a history that shrinks while the reader is away', async ({
  page,
}) => {
  await test.step('renders a transcript that arrives under the working indicator', async () => {
    await page.goto('/?fixture=long-transcript&count=0');
    await expect(page.getByTestId('fixture-count')).toHaveText('0 messages');
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(true),
    );
    await expect(page.getByRole('status', { name: 'Working' })).toBeVisible();
    await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__?.hydrate(40));
    await expect(page.getByTestId('fixture-count')).toHaveText('40 messages');
    await expect
      .poll(async () => (await viewportState(page)).gap)
      .toBeLessThanOrEqual(0);
    await expect(page.locator('[data-index="39"]')).toBeVisible();
  });

  await test.step('lands on rendered content in a session that shrank while away', async () => {
    await page.evaluate(() => {
      window.__TAU_TRANSCRIPT_FIXTURE__?.setWorking(false);
      window.__TAU_TRANSCRIPT_FIXTURE__?.hydrate(120);
    });
    await expect(page.getByTestId('fixture-count')).toHaveText('120 messages');
    const transcript = page.getByLabel('Transcript');
    await transcript.hover();
    for (let step = 0; step < 4; step += 1) {
      await page.mouse.wheel(0, -600);
      await page.waitForTimeout(60);
    }
    const parked = await viewportState(page);
    expect(parked.scrollTop).toBeGreaterThan(0);
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.switchSession('elsewhere'),
    );
    await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__?.truncate(4));
    await expect(page.getByTestId('fixture-count')).toHaveText('4 messages');
    await page.evaluate(() =>
      window.__TAU_TRANSCRIPT_FIXTURE__?.switchSession('main'),
    );
    await expect
      .poll(async () => (await viewportState(page)).gap)
      .toBeLessThanOrEqual(0);
    expect((await viewportState(page)).firstIndex).toBe(0);
  });
});
