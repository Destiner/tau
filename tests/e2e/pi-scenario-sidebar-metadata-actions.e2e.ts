import { expect, test } from './fixtures';

test.use({ pausedClock: true });

const promptTitle = 'Describe the metadata change';
const answer = 'Archive the selected sidebar metadata.';

test('sidebar metadata actions remain targeted while an extension editor waits', async ({
  page,
}) => {
  await page.goto('/?test-scenario=cross-project-sidebar-metadata-actions');
  const prompt = page.getByRole('dialog', { name: promptTitle });
  const editor = prompt.getByRole('textbox', { name: promptTitle });
  await expect(prompt).toBeVisible();
  await editor.fill(answer);
  const main = page.locator('.session-row[data-session-id="session-main"]');
  const backup = page.locator('.session-row[data-session-id="session-backup"]');
  const other = page.locator(
    '.session-row[data-session-id="session-other-project"]',
  );
  await expect(main).toHaveClass(/selected/);

  async function toggleRead(row: typeof main): Promise<void> {
    await row.click({ button: 'right' });
    const unread = page.getByRole('menuitem', { name: 'Mark as Unread' });
    await expect(unread).toBeEnabled();
    await unread.click();
    await row.click({ button: 'right' });
    const read = page.getByRole('menuitem', { name: 'Mark as Read' });
    await expect(read).toBeEnabled();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menuitem')).toHaveCount(0);
    await page.clock.runFor(1);
    await expect(row.locator('.session-select')).toBeFocused();
    await expect(editor).toHaveValue(answer);
    await row.click({ button: 'right' });
    await read.click();
    await row.click({ button: 'right' });
    await expect(
      page.getByRole('menuitem', { name: 'Mark as Unread' }),
    ).toBeEnabled();
    await page.keyboard.press('Escape');
  }

  await toggleRead(backup);
  await toggleRead(other);
  await toggleRead(main);
  await expect(main).toHaveClass(/selected/);

  await backup.click({ button: 'right' });
  await expect(
    page.getByRole('menuitem', { name: 'Archive Session' }),
  ).toBeEnabled();
  await page.getByRole('menuitem', { name: 'Archive Session' }).click();
  await expect(backup).toHaveCount(0);
  await expect(other).toBeVisible();
  await expect(main).toHaveClass(/selected/);
  await expect(editor).toHaveValue(answer);

  await other.hover();
  await other.getByRole('button', { name: 'Archive Other project' }).click();
  await expect(other).toHaveCount(0);
  await expect(main).toHaveClass(/selected/);
  await expect(editor).toHaveValue(answer);
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  const archived = page.locator('.archived-list');
  await expect(archived).toContainText('Backup');
  await expect(archived).toContainText('Other project');
  await expect(archived).not.toContainText('Main');
  const otherArchived = archived.locator('.row', { hasText: 'Other project' });
  await otherArchived.hover();
  await otherArchived
    .getByRole('button', { name: 'Unarchive Other project' })
    .click();
  await expect(otherArchived).toHaveCount(0);
  await page.getByRole('button', { name: 'Show Sessions' }).click();
  await expect(other).toBeVisible();
  await expect(backup).toHaveCount(0);
  await expect(main).toHaveClass(/selected/);
  await expect(editor).toHaveValue(answer);
  await prompt.getByRole('button', { name: 'Submit' }).click();
  await expect(prompt).toHaveCount(0);
  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok, verification?.error).toBe(true);
});
