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

test('finds archived work from the sidebar menu, reviews and restores it', async ({
  page,
}) => {
  await test.step('opens the sidebar actions from empty project-list space', async () => {
    await page.goto(scenarioUrl);
    const projectList = page.locator('.projects-body');
    const activeRow = page.locator('.session-row');
    await activeRow.hover();
    await activeRow.getByRole('button', { name: 'Archive Main' }).hover();
    const archiveTooltip = page.locator('.ui-tooltip');
    await expect(archiveTooltip.locator('.ui-tooltip-label')).toHaveText(
      'Archive Session',
    );
    await activeRow.click({ button: 'right' });
    const sessionActions = page.getByRole('menuitem');
    await expect(
      sessionActions.locator('span').allTextContents(),
    ).resolves.toEqual(['Mark as Unread', 'Archive Session']);
    await expect(sessionActions.locator('.ui-menu-shortcut')).toHaveCount(2);
    await page.keyboard.press('Escape');
    const box = await projectList.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;
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
    const archivedRow = archivedList.locator('.row', {
      hasText: 'Older archived work',
    });
    await archivedRow.locator('.copy').click();
    await expect(archivedRow).toHaveClass(/selected/);
  });

  await test.step('reviews, opens, and unarchives an archived session', async () => {
    await expect(
      page.getByRole('textbox', { name: 'Message Pi' }),
    ).toBeEnabled();
    const archivedList = page.locator('.archived-list');
    await expect(archivedList).toBeVisible();
    await expect(page.locator('.project-group')).toHaveCount(0);
    await expect(
      archivedList.getByText('Today', { exact: true }),
    ).toBeVisible();
    await expect(archivedList.locator('.row')).toHaveCount(61);
    const row = archivedList.locator('.row', {
      hasText: 'Older archived work',
    });
    await expect(row).toContainText('Tau fixture');
    await expect(row).not.toContainText('56y');
    await expect(row).toContainText('alpha');
    await row.locator('.copy').click();
    await expect(
      page.getByRole('textbox', { name: 'Message Pi' }),
    ).toBeEnabled();
    await expect(row).toHaveClass(/selected/);
    await row.hover();
    const unarchiveButton = row.getByRole('button', {
      name: 'Unarchive Older archived work',
    });
    await unarchiveButton.hover();
    const unarchiveTooltip = page.locator('.ui-tooltip');
    await expect(unarchiveTooltip.locator('.ui-tooltip-label')).toHaveText(
      'Unarchive Session',
    );
    await unarchiveButton.click();
    await expect(row).toHaveCount(0);
    await expect(archivedList.locator('.row')).toHaveCount(60);
    await page.getByRole('button', { name: 'Show Sessions' }).click();
    await expect(
      page.locator('.session-row').filter({ hasText: 'Older archived work' }),
    ).toBeVisible();
  });
});
