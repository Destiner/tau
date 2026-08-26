import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-order';

async function rowTops(rows: Locator): Promise<Record<string, number>> {
  return await rows.evaluateAll((elements) =>
    Object.fromEntries(
      elements.map((element) => [
        element.querySelector('.session-title')?.textContent ?? '',
        element.getBoundingClientRect().top,
      ]),
    ),
  );
}

async function projectTops(page: Page): Promise<Record<string, number>> {
  return await page
    .locator('.project-group')
    .evaluateAll((elements) =>
      Object.fromEntries(
        elements.map((element) => [
          element.querySelector('.project-toggle span')?.textContent ?? '',
          element.getBoundingClientRect().top,
        ]),
      ),
    );
}

async function mutateWhileHeld(page: Page): Promise<void> {
  await page.evaluate(() => {
    const fixture = window.__TAU_SESSION_ORDER_FIXTURE__;
    if (!fixture) throw new Error('Expected session-order fixture API.');
    fixture.updateActivity('/fixture/alpha', 'Alpha older', 100);
    fixture.materializeSession(
      '/fixture/alpha',
      'Alpha recent',
      'Alpha materialized id',
    );
    fixture.addSession('/fixture/alpha', 'Alpha newest', 110);
    fixture.addSession('/fixture/empty', 'Empty newest', 120);
  });
}

test('holds every existing row while new sessions and activity arrive', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  const rows = page.locator('.session-row');
  await expect(rows).toHaveCount(4);
  const initialRows = await rowTops(rows);
  const initialProjects = await projectTops(page);

  // Enter through a nested title, as a real pointer does when aiming at a row.
  await page.getByText('Alpha older', { exact: true }).hover();
  await mutateWhileHeld(page);

  await expect(rows).toHaveCount(4);
  await expect(page.getByText('Alpha newest', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Empty newest', { exact: true })).toHaveCount(0);
  expect(await rowTops(rows)).toEqual(initialRows);
  expect(await projectTops(page)).toEqual(initialProjects);

  // Moving between nested descendants must not re-capture a partially updated
  // order or release the hold.
  await page.getByText('Beta older', { exact: true }).hover();
  await page.evaluate(() => {
    const fixture = window.__TAU_SESSION_ORDER_FIXTURE__;
    if (!fixture) throw new Error('Expected session-order fixture API.');
    fixture.updateActivity('/fixture/beta', 'Beta older', 130);
    fixture.addSession('/fixture/beta', 'Beta newest', 140);
  });
  await expect(rows).toHaveCount(4);
  expect(await rowTops(rows)).toEqual(initialRows);
  expect(await projectTops(page)).toEqual(initialProjects);

  await page.getByTestId('outside-sidebar').hover();
  await expect(rows).toHaveCount(7);
  await expect(page.getByText('Alpha newest', { exact: true })).toBeVisible();
  await expect(page.getByText('Empty newest', { exact: true })).toBeVisible();
  await expect(page.getByText('Beta newest', { exact: true })).toBeVisible();

  const titles = await rows.locator('.session-title').allTextContents();
  expect(titles).toEqual([
    'Alpha newest',
    'Alpha older',
    'Alpha recent',
    'Empty newest',
    'Beta newest',
    'Beta older',
    'Beta recent',
  ]);
});
