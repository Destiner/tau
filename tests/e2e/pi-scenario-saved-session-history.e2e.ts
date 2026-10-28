import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

/** How much blank canvas sits above the topmost rendered row. */
async function gapAboveFirstRow(page: Page): Promise<number> {
  return await page.evaluate(() => {
    const element = document.querySelector('[aria-label="Transcript"]');
    if (!element) return -1;
    const tops = Array.from(document.querySelectorAll('[data-index]')).map(
      (row) => row.getBoundingClientRect().top,
    );
    if (tops.length === 0) return -1;
    return Math.round(
      Math.min(...tops) - element.getBoundingClientRect().top - 28,
    );
  });
}

test('renders the start of a long history the reader scrolls back to', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-long-history');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const transcript = page.getByLabel('Transcript');
  await expect(transcript).toContainText('Reply 59.');

  // Reading back through rows that were only ever estimated is where a window
  // drawn from the wrong offset shows up as blank canvas rather than history.
  await transcript.hover();
  for (let step = 0; step < 60; step += 1) {
    await page.mouse.wheel(0, -400);
    expect(await gapAboveFirstRow(page)).toBeLessThanOrEqual(0);
    if ((await transcript.evaluate((element) => element.scrollTop)) === 0)
      break;
  }

  await expect(
    page.getByText("I'll start by reading the issue.", { exact: true }),
  ).toBeVisible();
});
