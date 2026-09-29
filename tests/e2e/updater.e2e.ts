import appVersion from '../../src/lib/app-version';

import { expect, test } from './fixtures';

async function finishCheck(
  page: import('@playwright/test').Page,
  fail: boolean,
): Promise<void> {
  await expect(
    page.locator('html[data-update-check-pending="true"]'),
  ).toHaveCount(1);
  await page.evaluate((shouldFail) => {
    window.dispatchEvent(
      new CustomEvent('tau:test-update-check', {
        detail: { fail: shouldFail },
      }),
    );
  }, fail);
}

test('keeps failed checks inside the sidebar popover before and after viewing, then recovers', async ({
  page,
}) => {
  await page.goto('/?test-update=controlled');
  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  const popover = page.locator('.update-popover');
  await expect(trigger).toBeVisible();
  const before = await trigger.boundingBox();
  await finishCheck(page, true);
  await expect(popover).toHaveCount(0);
  await expect(trigger).toHaveAttribute(
    'aria-label',
    `Tau version ${appVersion}`,
  );
  await expect(
    trigger.locator('.update-indicator, .update-failure-mark'),
  ).toHaveCount(0);
  expect(await trigger.boundingBox()).toEqual(before);

  await trigger.click();
  await expect(popover.getByText('Update failed')).toBeVisible();
  await expect(
    popover.getByText(
      'Tau could not check for updates. Check your connection and try again.',
    ),
  ).toBeVisible();
  await expect(popover.getByRole('button', { name: 'Update' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveAttribute(
    'aria-label',
    `Tau version ${appVersion}`,
  );
  await trigger.click();
  await expect(popover.getByText('Update failed')).toBeVisible();

  await popover.getByRole('button', { name: 'Update' }).click();
  await expect(
    popover.getByRole('button', { name: 'Checking…' }),
  ).toBeDisabled();
  await finishCheck(page, false);
  await expect(popover.getByText('Version 0.2.0 available')).toBeVisible();
  await expect(trigger.locator('.update-indicator.accent')).toBeVisible();
});

test('shows a repeated check failure in place without changing the trigger', async ({
  page,
}) => {
  await page.goto('/?test-update=controlled');
  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  await trigger.click();
  const popover = page.locator('.update-popover');
  await finishCheck(page, true);
  await expect(popover.getByText('Update failed')).toBeVisible();
  await popover.getByRole('button', { name: 'Update' }).click();
  await finishCheck(page, true);
  await expect(popover.getByText('Update failed')).toBeVisible();
  await expect(popover.getByRole('button', { name: 'Update' })).toBeEnabled();
  await expect(trigger).toHaveAttribute(
    'aria-label',
    `Tau version ${appVersion}`,
  );
  await expect(
    trigger.locator('.update-indicator, .update-failure-mark'),
  ).toHaveCount(0);
  await expect(popover).toBeVisible();
});

test('reveals the popover for a native check and keeps its failure inside', async ({
  page,
}) => {
  await page.goto('/?test-update=controlled');
  await finishCheck(page, false);
  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  await expect(trigger.locator('.update-indicator.accent')).toBeVisible();
  await page.evaluate(async () => {
    const internals = window.__TAURI_INTERNALS__ as {
      invoke(command: string, args: Record<string, unknown>): Promise<unknown>;
    };
    await internals.invoke('plugin:event|emit', {
      event: 'tau://check-for-updates',
      payload: null,
    });
  });
  const popover = page.locator('.update-popover');
  await expect(popover).toBeVisible();
  await finishCheck(page, true);
  await expect(popover.getByText('Update failed')).toBeVisible();
  await expect(trigger).toHaveAttribute(
    'aria-label',
    `Tau version ${appVersion}`,
  );
  await expect(trigger.locator('.update-failure-mark')).toHaveCount(0);
});

test('keeps download and verification failures local and actionable', async ({
  page,
}) => {
  for (const [fixture, copy] of [
    [
      'download-failure',
      'The update could not be downloaded. Check your connection and try again.',
    ],
    [
      'verification-failure',
      'The update could not be verified and was not installed.',
    ],
  ] as const) {
    await page.goto(`/?test-update=${fixture}`);
    const trigger = page.getByRole('button', {
      name: `Tau version ${appVersion}`,
    });
    await trigger.click();
    const popover = page.locator('.update-popover');
    await popover.getByRole('button', { name: 'Update' }).click();
    await expect(popover.getByText(copy)).toBeVisible();
    await expect(popover.getByRole('button', { name: 'Update' })).toBeEnabled();
    await expect(trigger).toHaveAttribute(
      'aria-label',
      `Tau version ${appVersion}`,
    );
    await expect(trigger.locator('.update-failure-mark')).toHaveCount(0);
  }
});

test('keeps restart recovery in the popover after a failed invocation', async ({
  page,
}) => {
  await page.goto('/?test-update=restart-failure');
  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  await trigger.click();
  const popover = page.locator('.update-popover');
  await expect(popover.getByText('Restart needed')).toBeVisible();
  await popover.getByRole('button', { name: 'Restart' }).click();
  await expect(
    popover.getByText('The update could not be installed. Try again.'),
  ).toBeVisible();
  await expect(popover.getByRole('button', { name: 'Restart' })).toBeVisible();
  await expect(trigger).toHaveAttribute(
    'aria-label',
    `Tau version ${appVersion}`,
  );
  await popover.getByRole('button', { name: 'Restart' }).click();
  await expect(popover.getByText('Restart needed')).toBeVisible();
});

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

test('keeps update popover geometry stable while checking', async ({
  page,
}) => {
  await page.goto('/?test-update=checking');

  const trigger = page.getByRole('button', {
    name: `Tau version ${appVersion}`,
  });
  await trigger.click();
  const popover = page.locator('.update-popover');
  await popover.getByRole('button', { name: 'Skip' }).click();
  await trigger.click();
  const copy = popover.locator('.update-copy');
  await expect(copy.locator('strong')).toHaveText('Tau is up to date');
  await expect(copy.locator('span')).toHaveText(
    `Version ${appVersion} is installed.`,
  );
  const boxBeforeCheck = await popover.boundingBox();

  await popover.getByRole('button', { name: 'Check for Updates' }).click();
  const checking = popover.getByRole('button', { name: 'Checking…' });
  await expect(checking).toBeDisabled();
  await expect(copy.locator('strong')).toHaveText('Tau is up to date');
  await expect(copy.locator('span')).toHaveText(
    `Version ${appVersion} is installed.`,
  );
  await expect(popover).toHaveAttribute('aria-busy', 'true');
  expect(await popover.boundingBox()).toEqual(boxBeforeCheck);

  await expect(popover.getByText('Version 0.2.0 available')).toBeVisible();
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
