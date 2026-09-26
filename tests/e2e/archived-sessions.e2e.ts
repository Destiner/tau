import { expect, test } from './fixtures';

/**
 * The archived-sessions view is a workspace-wide review surface: the footer
 * toggle swaps the project list for a flat, time-grouped list, rows open
 * read-only, and a session comes back either through the unarchive action or
 * by sending it a message.
 */
const scenarioUrl = '/?test-scenario=archived-sessions-review';

async function revealOlderRow(
  page: import('@playwright/test').Page,
): Promise<void> {
  const list = page.locator('.archived-list');
  await expect(list.locator('.row')).toHaveCount(50);
  await list.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(list.locator('.row')).toHaveCount(61);
}

test('opens the sidebar actions from empty project-list space', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const projectList = page.locator('.projects-body');

  // A session keeps its own menu rather than inheriting the empty-space menu.
  await page.locator('.session-row').click({ button: 'right' });
  const sessionActions = page.getByRole('menuitem');
  await expect(sessionActions).toHaveText([
    'Mark as Unread',
    'Archive Session',
  ]);
  await page.keyboard.press('Escape');

  const box = await projectList.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;

  // This is below the fixture's project row and clear of the resize handle.
  await projectList.click({
    button: 'right',
    position: { x: Math.min(100, box.width / 2), y: box.height - 4 },
  });

  const actions = page.getByRole('menuitem');
  await expect(actions).toHaveText([
    'Open Local Project',
    'Open Remote Project',
    'Show Archived Sessions',
  ]);

  await actions.filter({ hasText: 'Show Archived Sessions' }).click();
  const archivedList = page.locator('.archived-list');
  await expect(archivedList).toBeVisible();

  await revealOlderRow(page);
  // Opening the archived row consumes this scenario's second Pi runtime.
  const archivedRow = archivedList.locator('.row', {
    hasText: 'Older archived work',
  });
  await archivedRow.locator('.copy').click();
  await expect(archivedRow).toHaveClass(/selected/);
  await archivedRow.hover();
  await archivedRow
    .getByRole('button', { name: 'Unarchive Older archived work' })
    .click();
  await expect(archivedRow).toHaveCount(0);
  await expect(archivedList.locator('.row')).toHaveCount(60);
});

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
  await revealOlderRow(page);
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
  await expect(row).toHaveCount(0);
  await expect(archivedList.locator('.row')).toHaveCount(60);

  await page.getByRole('button', { name: 'Show Sessions' }).click();
  await expect(
    page.locator('.session-row').filter({ hasText: 'Older archived work' }),
  ).toBeVisible();
});
