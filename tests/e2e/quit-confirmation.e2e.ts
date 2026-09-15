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

for (const colorScheme of ['light', 'dark'] as const) {
  test(`dialog actions have equal bounds and accent fill in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await page.goto(fixtureUrl);
    const quit = page.getByRole('button', { name: 'Quit', exact: true });
    const cancel = page.getByRole('button', { name: 'Cancel' });
    await expect(quit).toBeFocused();
    const quitBounds = await quit.boundingBox();
    const cancelBounds = await cancel.boundingBox();
    expect(quitBounds?.height).toBe(28);
    expect(quitBounds?.height).toBe(cancelBounds?.height);
    expect(quitBounds?.y).toBe(cancelBounds?.y);
    const accent = await quit.evaluate((button) => {
      const probe = document.createElement('span');
      probe.style.color = 'var(--accent)';
      button.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    });
    await expect(quit).toHaveCSS('background-color', accent);
    await expect(quit).toHaveCSS('box-shadow', /inset/);
    await cancel.focus();
    await expect(cancel).toBeFocused();
    await expect(cancel).toHaveCSS('box-shadow', /inset/);
  });
}

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
