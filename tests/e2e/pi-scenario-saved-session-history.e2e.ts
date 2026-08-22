import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

/**
 * A session opened after it was created elsewhere — by a workflow extension,
 * or on another machine — hands Tau a transcript that is already complete, so
 * its first render is the whole thing at once rather than a turn arriving row
 * by row. That is the render this covers: nothing else opens a session that
 * holds a transcript, since every other scenario bootstraps into an empty one.
 */
const scenarioUrl = '/?test-scenario=saved-session-history';

/** How much blank canvas sits above the topmost rendered row. */
async function gapAboveFirstRow(page: Page): Promise<number> {
  return await page.evaluate(() => {
    const element = document.querySelector('[aria-label="Tau transcript"]');
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

test('opens a session that already holds a transcript at its end', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const transcript = page.getByLabel('Tau transcript');
  await expect(transcript).toContainText('Reply 11.');

  // The reader arrives at the end of the transcript, on rendered content: a
  // window drawn from anywhere else leaves blank canvas above its first row.
  expect(await gapAboveFirstRow(page)).toBeLessThanOrEqual(0);
  await expect(
    page.getByText('History prompt 11', { exact: true }),
  ).toBeVisible();

  // The rows the reader scrolls back to are there without a nudge to force it.
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByText("I'll start by reading the issue.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
  expect(await gapAboveFirstRow(page)).toBeLessThanOrEqual(0);
});

test('renders the start of a long history the reader scrolls back to', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-long-history');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const transcript = page.getByLabel('Tau transcript');
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

test('shows the whole transcript of a session too short to scroll', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-short-history');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("I'll start by reading the issue.", { exact: true }),
  ).toBeVisible();
  expect(await gapAboveFirstRow(page)).toBeLessThanOrEqual(0);
});
