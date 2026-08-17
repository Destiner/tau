import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=issue-report';

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
});

test('submits an issue with optional session context', async ({ page }) => {
  const trigger = page.getByRole('button', { name: 'Report an issue' });
  await trigger.click();

  const dialog = page.getByRole('dialog', { name: 'Report an issue' });
  const includeSession = dialog.getByRole('checkbox', {
    name: 'Include current session',
  });
  const description = dialog.getByRole('textbox', {
    name: 'Describe the issue',
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

test('keeps the description available after dismissal or failure', async ({
  page,
}) => {
  const trigger = page.getByRole('button', { name: 'Report an issue' });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: 'Report an issue' });
  const description = dialog.getByRole('textbox', {
    name: 'Describe the issue',
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
