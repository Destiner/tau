import { expect, test } from './fixtures';

/**
 * The archived-sessions view is a workspace-wide review surface: the footer
 * toggle swaps the project list for a flat, time-grouped list, rows open
 * read-only, and a session comes back either through the unarchive action or
 * by sending it a message.
 */
const scenarioUrl = '/?test-scenario=archived-sessions-review';

test('reviews, opens, and unarchives an archived session', async ({ page }) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();

  // The fixture's agent-only activity has no user-message timestamp, but its
  // current sort timestamp still places it in Today and drives its display.
  const archivedList = page.locator('.archived-list');
  await expect(archivedList).toBeVisible();
  await expect(page.locator('.project-group')).toHaveCount(0);
  await expect(archivedList.getByText('Today', { exact: true })).toBeVisible();
  const row = archivedList.locator('.row', {
    hasText: 'Older archived work',
  });
  await expect(row).toContainText('Tau fixture');
  await expect(row).not.toContainText('56y');
  await expect(row).toContainText('alpha');

  // Opening browses the transcript without resurrecting the session: the
  // record stays archived and only a send restores it.
  await row.locator('.copy').click();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  // The opened session is highlighted in the archived list, same as the
  // active session in the project lists.
  await expect(row).toHaveClass(/selected/);

  // The explicit action restores the session into its project's own list.
  await row.hover();
  await row
    .getByRole('button', { name: 'Unarchive Older archived work' })
    .click();
  await expect(archivedList.getByText('No archived sessions')).toBeVisible();

  await page.getByRole('button', { name: 'Show Sessions' }).click();
  await expect(
    page.locator('.session-row').filter({ hasText: 'Older archived work' }),
  ).toBeVisible();
});
