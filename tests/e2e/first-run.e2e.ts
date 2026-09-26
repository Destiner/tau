import appVersion from '../../src/lib/app-version';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=empty-workspace';

test('shows preparation feedback while ownership is pending', async ({
  page,
}) => {
  await page.goto(`${scenarioUrl}&ownership-delay=3000`);

  await expect(page.locator('.first-run-preparing')).toContainText(
    'Preparing Pi',
  );
  await expect(
    page.getByRole('button', { name: 'Open Local Project' }),
  ).toHaveCount(0);
  const localProject = page.getByRole('button', {
    name: 'Open Local Project',
  });
  await expect(localProject).toBeVisible();
  await expect(page.locator('.first-run-preparing')).toHaveCount(0);
  await localProject.click();
  await page.getByRole('button', { name: 'Open Remote Project' }).click();
  await expect(
    page.getByRole('dialog', { name: 'SSH Connection' }),
  ).toBeVisible();
});

test('offers project actions across the empty workspace', async ({ page }) => {
  await page.goto(scenarioUrl);

  await expect(page.getByLabel(`Tau version ${appVersion}`)).toBeVisible();
  await expect(page.getByText('tau', { exact: true })).toBeVisible();
  await expect(page.getByText(appVersion, { exact: true })).toBeVisible();
  await expect(page.getByText('Open a project', { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByText('Choose a local folder or connect over SSH.'),
  ).toHaveCount(0);
  await expect(page.getByLabel('Projects and Sessions')).toHaveCount(0);
  await expect(page.locator('.session-header')).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toHaveCount(
    0,
  );

  const localProject = page.getByRole('button', {
    name: 'Open Local Project',
  });
  const remoteProject = page.getByRole('button', {
    name: 'Open Remote Project',
  });
  await expect(localProject).toBeFocused();
  await expect(localProject).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(remoteProject).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const localBox = await localProject.boundingBox();
  const remoteBox = await remoteProject.boundingBox();
  expect(localBox).not.toBeNull();
  expect(remoteBox).not.toBeNull();
  if (localBox && remoteBox) {
    expect(remoteBox.x).toBeGreaterThanOrEqual(localBox.x + localBox.width);
    expect(remoteBox.y + remoteBox.height / 2).toBeCloseTo(
      localBox.y + localBox.height / 2,
      0,
    );
  }

  await localProject.click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await remoteProject.click();

  const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(remoteProject).toBeFocused();
});
