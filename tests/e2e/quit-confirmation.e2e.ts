import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=quit-confirmation';

test('confirms an interrupted quit from the keyboard', async ({ page }) => {
  await page.goto(fixtureUrl);

  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading')).toHaveText('Quit Tau?');
  await expect(dialog).toContainText('2 sessions are still in progress.');
  await expect(dialog.getByRole('button', { name: 'Quit' })).toBeFocused();

  await page.keyboard.press('Enter');

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('quit-outcome')).toHaveText('confirmed');
});

test('confirms an interrupted quit with Mod+Enter', async ({ page }) => {
  await page.goto(fixtureUrl);

  const dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: 'Quit' }).press('Meta+Enter');

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('quit-outcome')).toHaveText('confirmed');
});

test('cancels the interrupted quit with Escape', async ({ page }) => {
  await page.goto(`${fixtureUrl}&sessions=1`);

  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText('One session is still in progress.');
  const cancel = dialog.getByRole('button', { name: 'Cancel' });
  await cancel.focus();
  await expect(cancel).toBeFocused();

  await page.evaluate(() => {
    document.addEventListener('keydown', () => {
      document.body.dataset.escaped = 'true';
    });
  });
  const prevented = await page.evaluate(() => {
    const event = new KeyboardEvent('keydown', {
      key: 'Escape',
      bubbles: true,
      cancelable: true,
    });
    document.activeElement?.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(true);
  await expect(page.locator('body')).not.toHaveAttribute('data-escaped');

  await expect(dialog).toBeHidden();
  await expect(page.getByTestId('quit-outcome')).toHaveText('cancelled');

  // Closing the modal releases Escape back to the app.
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).toHaveAttribute('data-escaped', 'true');
});

test('busy quit consumes Escape without cancelling', async ({ page }) => {
  await page.goto(`${fixtureUrl}&busy`);
  await expect(page.getByRole('alertdialog')).toBeVisible();
  await page.evaluate(() => {
    document.addEventListener('keydown', () => {
      document.body.dataset.escaped = 'true';
    });
  });
  await page.keyboard.press('Escape');
  await expect(page.getByRole('alertdialog')).toBeVisible();
  await expect(page.getByTestId('quit-outcome')).toBeEmpty();
  await expect(page.locator('body')).not.toHaveAttribute('data-escaped');
});
