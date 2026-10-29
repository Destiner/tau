import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

async function connectLinear(dialog: Locator): Promise<void> {
  await dialog
    .getByRole('textbox', { name: 'API Key', exact: true })
    .fill('fixture-key');
  await dialog.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(
    dialog.getByRole('combobox', { name: 'Team', exact: true }),
  ).toBeVisible();
  await dialog
    .getByRole('button', { name: 'Save Destination', exact: true })
    .click();
}

async function openSetup(page: Page): Promise<Locator> {
  await page.goto('/?fixture=issue-report&setup=true&slow=true');
  await page
    .getByRole('button', { name: 'Report an Issue', exact: true })
    .click();
  return page.getByRole('dialog', { name: 'Report an Issue', exact: true });
}

test('sets up Linear step by step and can report despite Grafana failure', async ({
  page,
}) => {
  const dialog = await openSetup(page);
  const key = dialog.getByRole('textbox', { name: 'API Key', exact: true });
  await expect(key).toBeVisible();
  expect(await key.evaluate((element) => element.tagName)).toBe('TEXTAREA');
  await expect(key).toHaveCSS('-webkit-text-security', 'none');
  await expect(key).toHaveAttribute('autocomplete', 'off');
  await key.fill('fake-\nkey');
  await expect(key).toHaveValue('fake-key');
  await expect(dialog.locator('.ui-token-mask').first()).toHaveText('••••••••');
  await expect(key).toHaveCSS('color', 'rgba(0, 0, 0, 0)');
  expect(
    await key.evaluate((element) => getComputedStyle(element).caretColor),
  ).not.toBe('rgba(0, 0, 0, 0)');
  await connectLinear(dialog);
  await expect(
    dialog.getByRole('heading', { name: 'Grafana', exact: true }),
  ).toBeVisible();
  await dialog
    .getByLabel('OTLP Endpoint')
    .fill('https://otlp.example.invalid/otlp');
  await dialog.getByLabel('Instance ID').fill('123');
  await dialog.getByRole('textbox', { name: 'Token', exact: true }).fill('bad');
  await dialog.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText(
    'Check the Grafana credentials and permissions.',
  );
  await dialog.getByRole('button', { name: 'Skip', exact: true }).click();
  await dialog
    .getByRole('textbox', { name: 'Describe the Issue' })
    .fill('Report without telemetry');
  await dialog.getByRole('button', { name: 'Submit', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('submitted-report')).toHaveText(
    '{"description":"Report without telemetry"}',
  );
});

test('uses one reset icon per provider without losing the report', async ({
  page,
}) => {
  const dialog = await openSetup(page);
  await connectLinear(dialog);
  await dialog
    .getByLabel('OTLP Endpoint')
    .fill('https://otlp.example.invalid/otlp');
  await dialog.getByLabel('Instance ID').fill('123');
  await dialog
    .getByRole('textbox', { name: 'Token', exact: true })
    .fill('fixture-token');
  await dialog.getByRole('button', { name: 'Connect', exact: true }).click();
  await dialog.getByRole('button', { name: 'Continue', exact: true }).click();
  await dialog
    .getByRole('textbox', { name: 'Describe the Issue' })
    .fill('Keep this draft');
  await dialog
    .getByRole('button', { name: 'Connections', exact: true })
    .click();
  await expect(dialog.getByRole('button', { name: /Replace/ })).toHaveCount(0);
  await dialog
    .getByRole('button', { name: 'Disconnect Grafana', exact: true })
    .click();
  await expect(dialog.getByLabel('OTLP Endpoint')).toHaveValue('');
  await expect(dialog.getByLabel('Instance ID')).toHaveValue('');
  await expect(
    dialog.getByRole('textbox', { name: 'Token', exact: true }),
  ).toHaveValue('');
  await expect(
    dialog.getByRole('button', { name: 'Done', exact: true }),
  ).toBeEnabled();
  await dialog
    .getByRole('button', { name: 'Disconnect Linear', exact: true })
    .click();
  await expect(
    dialog.getByRole('textbox', { name: 'API Key', exact: true }),
  ).toHaveValue('');
  await expect(
    dialog.getByRole('button', { name: 'Done', exact: true }),
  ).toBeDisabled();
  const linear = dialog.locator('section').filter({
    has: page.getByRole('heading', { name: 'Linear', exact: true }),
  });
  await connectLinear(linear);
  await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  await expect(
    dialog.getByRole('textbox', { name: 'Describe the Issue' }),
  ).toHaveValue('Keep this draft');
});

test('keeps the reporter above the hovered sidebar divider', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-bootstrap');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page.getByLabel('Projects and Sessions').click();
  await page.keyboard.type('iddqd');
  await page
    .getByRole('button', { name: 'Report an Issue', exact: true })
    .click();
  const dialog = page.getByRole('dialog', {
    name: 'Report an Issue',
    exact: true,
  });
  await expect(
    dialog.getByRole('textbox', { name: 'Describe the Issue' }),
  ).toBeVisible();
  const handle = await page.locator('.sidebar-resize-handle').boundingBox();
  const surface = await dialog.boundingBox();
  if (!handle || !surface) throw new Error('Missing sidebar/reporter geometry');
  const x = handle.x + handle.width / 2;
  await page.mouse.move(x, surface.y - 8);
  expect(
    await page.evaluate(
      ({ x, y }) =>
        Boolean(
          document.elementFromPoint(x, y)?.closest('.issue-report-popover'),
        ),
      { x, y: surface.y + 30 },
    ),
  ).toBe(true);
});
