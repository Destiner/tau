import { expect, test } from './fixtures';

test('project context actions target an inactive project and restore focus', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const group = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  const projectSelect = group.locator('.project-select');
  await projectSelect.click({ button: 'right' });
  const newSession = page.getByRole('menuitem', {
    name: 'New Session',
    exact: true,
  });
  const remove = page.getByRole('menuitem', {
    name: 'Remove Project',
    exact: true,
  });
  const rename = page.getByRole('menuitem', {
    name: 'Rename Project…',
    exact: true,
  });
  await expect(page.getByRole('menuitem').locator(':scope > span')).toHaveText([
    'New Session',
    'Rename Project…',
    'Remove Project',
  ]);
  await newSession.hover();
  await page.keyboard.press('ArrowDown');
  await expect(rename).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(remove).toBeFocused();
  await expect(group.locator('.session-list')).toBeVisible();
  await expect(group).not.toHaveClass(/project-sortable-chosen/);
  await page.keyboard.press('Escape');
  await expect(projectSelect).toBeFocused();

  await projectSelect.click({ button: 'right' });
  await remove.click();
  const dialog = page.locator('.project-removal');
  await expect(dialog).toContainText('“notes”');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(projectSelect).toBeFocused();
});
