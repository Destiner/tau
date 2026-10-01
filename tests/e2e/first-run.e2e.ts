import appVersion from '../../src/lib/app-version';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=empty-workspace';

test('keeps the home version a static informational label', async ({
  page,
}) => {
  await page.goto(`${scenarioUrl}&test-update=first-run-failure`);

  const home = page.locator('.first-run');
  await expect(home.getByText('tau', { exact: true })).toBeVisible();
  await expect(home.getByText(appVersion, { exact: true })).toBeVisible();
  await expect(
    page.getByRole('button', { name: `Tau version ${appVersion}` }),
  ).toHaveCount(0);
  await expect(page.locator('.update-popover')).toHaveCount(0);
  const label = home.locator('.first-run-version');
  await expect(label).toHaveText(`tau${appVersion}`);
  await expect(label).not.toHaveAttribute('tabindex');
  const background = await label.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  await label.hover();
  await expect(label).toHaveCSS('background-color', background);
  await label.click();
  await expect(page.locator('.update-popover')).toHaveCount(0);
  await page.keyboard.press('Tab');
  await expect(label).not.toBeFocused();

  // The empty-workspace scenario expects its cancelled native chooser.
  await page.getByRole('button', { name: 'Open Local Project' }).click();
});

test('prepares Pi then offers and navigates both project actions in the empty workspace', async ({
  page,
}) => {
  await test.step('shows preparation feedback while ownership is pending', async () => {
    await page.clock.install();
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
    await page.clock.fastForward(3_000);
    await expect(localProject).toBeVisible();
    await expect(localProject).toBeFocused();
    await expect(page.locator('.first-run-preparing')).toHaveCount(0);
    await localProject.click();
    await expect(page.getByRole('alert')).toHaveCount(0);
    await page.getByRole('button', { name: 'Open Remote Project' }).click();
    await expect(
      page.getByRole('dialog', { name: 'SSH Connection' }),
    ).toBeVisible();
  });

  await test.step('returns to home after closing remote project', async () => {
    await page.keyboard.press('Escape');
    await expect(
      page.getByRole('dialog', { name: 'SSH Connection' }),
    ).toHaveCount(0);
    await page.mouse.move(0, 0);
    await expect(page.locator('.first-run-version')).toBeVisible();
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
    await expect(localProject).toHaveCSS(
      'background-color',
      'rgba(0, 0, 0, 0)',
    );
    await expect(remoteProject).toHaveCSS(
      'background-color',
      'rgba(0, 0, 0, 0)',
    );
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
    await remoteProject.click();
    const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
    await expect(dialog).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(remoteProject).toBeFocused();
  });
});
