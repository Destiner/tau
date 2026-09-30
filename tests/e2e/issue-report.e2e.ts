import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=issue-report';

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
});

for (const shortcut of ['Meta+Enter', 'Control+Enter'] as const) {
  test(`submits an issue once with ${shortcut}`, async ({ page }) => {
    const trigger = page.getByRole('button', { name: 'Report an Issue' });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'Report an Issue' });
    await dialog
      .getByRole('textbox', { name: 'Describe the Issue' })
      .fill('Shortcut submission');
    await dialog
      .getByRole('textbox', { name: 'Describe the Issue' })
      .press(shortcut);

    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId('submitted-report')).toHaveText(
      '{"description":"Shortcut submission"}',
    );
  });
}

for (const shortcut of ['Meta+Shift+S', 'Control+Shift+S'] as const) {
  test(`toggles session context with ${shortcut}`, async ({ page }) => {
    await page.getByRole('button', { name: 'Report an Issue' }).click();
    const dialog = page.getByRole('dialog', { name: 'Report an Issue' });
    const description = dialog.getByRole('textbox', {
      name: 'Describe the Issue',
    });
    const includeSession = dialog.getByRole('checkbox', {
      name: 'Include Current Session',
    });
    await description.fill('Shortcut session context');
    await description.press(shortcut);
    await expect(includeSession).toBeChecked();
    await description.press('Control+Enter');

    await expect(page.getByTestId('submitted-report')).toHaveText(
      '{"description":"Shortcut session context","sessionId":"fixture-session"}',
    );
  });
}

test('keeps an issue draft through dismissal and failure, then submits with and without session context', async ({
  page,
}) => {
  await test.step('keeps the description available after dismissal or failure', async () => {
    const trigger = page.getByRole('button', { name: 'Report an Issue' });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'Report an Issue' });
    const description = dialog.getByRole('textbox', {
      name: 'Describe the Issue',
    });
    await description.fill('Keep this draft');
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.click();
    await expect(description).toHaveValue('Keep this draft');
    await description.fill('fail');
    await dialog.getByRole('button', { name: 'Submit' }).click();
    await expect(dialog.getByRole('alert')).toHaveText(
      'The report could not be saved. Try again.',
    );
    await expect(description).toHaveValue('fail');
  });

  await test.step('submits an issue with optional session context', async () => {
    await page.keyboard.press('Escape');
    await expect(
      page.getByRole('dialog', { name: 'Report an Issue' }),
    ).toHaveCount(0);
    const trigger = page.getByRole('button', { name: 'Report an Issue' });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'Report an Issue' });
    const includeSession = dialog.getByRole('checkbox', {
      name: 'Include Current Session',
    });
    const description = dialog.getByRole('textbox', {
      name: 'Describe the Issue',
    });
    await expect(includeSession).not.toBeChecked();
    await description.fill('The sidebar stopped responding.');
    await dialog.getByRole('button', { name: 'Submit' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await expect(page.getByTestId('submitted-report')).toHaveText(
      '{"description":"The sidebar stopped responding."}',
    );
    await trigger.click();
    await expect(includeSession).not.toBeChecked();
    await includeSession.check();
    await description.fill('The reply appeared in the wrong session.');
    await dialog.getByRole('button', { name: 'Submit' }).click();
    await expect(page.getByTestId('submitted-report')).toHaveText(
      '{"description":"The reply appeared in the wrong session.","sessionId":"fixture-session"}',
    );
  });
});
