import { expect, test } from './fixtures';

test('project context actions target an inactive project and restore focus', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const group = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  const toggle = group.locator('.project-toggle');
  await toggle.click({ button: 'right' });
  const newSession = page.getByRole('menuitem', {
    name: 'New Session',
    exact: true,
  });
  const remove = page.getByRole('menuitem', {
    name: 'Remove Project',
    exact: true,
  });
  await expect(page.getByRole('menuitem')).toHaveCount(2);
  await newSession.hover();
  await page.keyboard.press('ArrowDown');
  await expect(remove).toBeFocused();
  await expect(group.locator('.session-list')).toBeVisible();
  await expect(group).not.toHaveClass(/project-sortable-chosen/);
  await page.keyboard.press('Escape');
  await expect(toggle).toBeFocused();

  await toggle.click({ button: 'right' });
  await remove.click();
  const dialog = page.locator('.project-removal');
  await expect(dialog).toContainText('“notes”');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(toggle).toBeFocused();
});

test('project menus do not replace session or background context menus', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page.locator('.session-row').first().click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: /Mark as/ })).toBeVisible();
  await expect(
    page.getByRole('menuitem', { name: 'Remove Project' }),
  ).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page
    .locator('.projects-body')
    .click({ button: 'right', position: { x: 12, y: 450 } });
  await expect(
    page.getByRole('menuitem', { name: 'Open Local Project' }),
  ).toBeVisible();
});
