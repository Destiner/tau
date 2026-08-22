import { expect, test } from './fixtures';

/**
 * The archived-sessions view is a workspace-wide review surface: the footer
 * toggle swaps the project list for a flat, time-grouped list, and unarchiving
 * returns the row to its project. Unarchiving is only offered for sessions
 * whose project Tau still has open — which, in this scenario, is all of them.
 */
const scenarioUrl = '/?test-scenario=archived-sessions-review';

test('toggles to the archived list and unarchives a session back into place', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const toggle = page.getByRole('button', {
    name: 'Show archived sessions',
  });
  await toggle.click();

  // The view replaces the project list and groups by time; the fixture
  // session's fixed past date always lands it in "Older".
  const archivedList = page.locator('.archived-list');
  await expect(archivedList).toBeVisible();
  await expect(page.locator('.project-group')).toHaveCount(0);
  await expect(archivedList.getByText('Older', { exact: true })).toBeVisible();
  const row = archivedList.locator('.row', {
    hasText: 'Older archived work',
  });
  await expect(row).toContainText('Tau fixture');
  await expect(row).toContainText('alpha');

  // The unarchive action reveals on hover and returns the session to the
  // project's own list.
  await row.hover();
  await row
    .getByRole('button', { name: 'Unarchive Older archived work' })
    .click();
  await expect(archivedList.getByText('No archived sessions')).toBeVisible();

  await page.getByRole('button', { name: 'Show sessions' }).click();
  await expect(
    page.locator('.session-row').filter({ hasText: 'Older archived work' }),
  ).toBeVisible();
});
