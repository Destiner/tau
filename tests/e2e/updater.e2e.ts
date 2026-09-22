import appVersion from '../../src/lib/app-version';

import { expect, test } from './fixtures';

test('downloads and installs an available update after idle auto-confirmation', async ({
  page,
}) => {
  await page.goto('/?test-update=available');

  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  const indicator = trigger.locator('.update-indicator');
  await expect(indicator).toHaveClass(/accent/);

  await trigger.click();
  const popover = page.locator('.update-popover');
  await popover.getByRole('button', { name: 'Update', exact: true }).click();

  await expect(popover.getByText('Downloading update')).toBeVisible();
  await expect(popover.getByRole('progressbar')).toBeVisible();
  await expect(indicator).toHaveClass(/downloading/);
  await expect(popover.getByText('Update ready')).toBeVisible();
  await expect(indicator).toHaveClass(/accent/);
  await Promise.all([
    page.waitForEvent('load'),
    popover.getByRole('button', { name: 'Restart', exact: true }).click(),
  ]);

  await expect(page.getByText('Restart Tau?')).toHaveCount(0);
  await expect(trigger.locator('.update-indicator')).toHaveCount(0);
  await trigger.click();
  await expect(
    page.locator('.update-popover').getByText('Tau is up to date'),
  ).toBeVisible();
});

test('shows and dismisses an available update from the version control', async ({
  page,
}) => {
  await page.goto('/?test-update=available');

  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  await expect(trigger).toBeVisible();
  await expect(trigger.locator('.update-indicator.accent')).toBeVisible();

  await trigger.click();
  const popover = page.locator('.update-popover');
  await expect(popover.getByText('Version 0.2.0 available')).toBeVisible();
  await expect(popover.getByRole('button', { name: 'Update' })).toBeVisible();

  await popover.getByRole('button', { name: 'Skip' }).click();
  await expect(popover).toBeHidden();
  await expect(trigger.locator('.update-indicator')).toHaveCount(0);

  await trigger.click();
  await expect(popover.getByText('Tau is up to date')).toBeVisible();
});
