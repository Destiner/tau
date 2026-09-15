import { expect, test } from '@playwright/test';

const fixtureUrl = '/?fixture=quit-confirmation';

test('confirms an interrupted quit from the keyboard', async ({ page }) => {
  await page.goto(fixtureUrl);

  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading')).toHaveText('Quit Tau?');
  await expect(dialog).toContainText(
    '2 sessions are still in progress. Quitting will stop them.',
  );
  await expect(dialog).toHaveCSS('top', '112px');
  await expect(dialog.getByRole('button', { name: 'Quit' })).toBeFocused();

  await page.keyboard.press('Enter');

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('quit-outcome')).toHaveText('confirmed');
});

test('cancels the interrupted quit with Escape', async ({ page }) => {
  await page.goto(`${fixtureUrl}&sessions=1`);

  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText(
    'One session is still in progress. Quitting will stop it.',
  );

  await page.keyboard.press('Escape');

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('quit-outcome')).toHaveText('cancelled');
});
