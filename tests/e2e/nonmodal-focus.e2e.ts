import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const url = '/?fixture=extension-dialog&prompt=compact&focusControls=true';

// Change async fixture state without moving focus like a real pointer click would.
async function trigger(page: Page, name: string): Promise<void> {
  await page
    .getByRole('button', { name, exact: true, includeHidden: true })
    .evaluate((button) => {
      (button as HTMLButtonElement).click();
    });
}

for (const method of ['select', 'confirm', 'input', 'editor']) {
  test(`${method} takes focus once and repairs only its own disabled focus`, async ({
    page,
  }) => {
    await page.goto(`${url}&method=${method}`);
    const prompt = page.locator('.extension-prompt');
    const control =
      method === 'select'
        ? prompt.getByRole('option').nth(1)
        : method === 'confirm'
          ? prompt.getByRole('button', { name: 'No', exact: true })
          : prompt.getByRole('textbox');
    await expect
      .poll(() =>
        prompt.evaluate((element) => element.contains(document.activeElement)),
      )
      .toBe(true);
    await control.focus();
    await trigger(page, 'Begin Submit');
    await expect(control).toBeDisabled();
    await trigger(page, 'Fail Submit');
    await expect(control).toBeFocused();

    await trigger(page, 'Toggle Disabled');
    await expect(control).toBeDisabled();
    await trigger(page, 'Toggle Disabled');
    await expect(control).toBeFocused();

    await trigger(page, 'Begin Submit');
    await expect(control).toBeDisabled();
    const unrelated = page.getByRole('textbox', { name: 'Unrelated input' });
    await unrelated.focus();
    await trigger(page, 'Fail Submit');
    await trigger(page, 'Update Prompt');
    await expect(unrelated).toBeFocused();

    // Moving away and then blurring is not focus lost by disabling the prompt.
    await control.focus();
    await trigger(page, 'Begin Submit');
    await expect(control).toBeDisabled();
    await unrelated.focus();
    await unrelated.evaluate((input) => (input as HTMLInputElement).blur());
    await trigger(page, 'Fail Submit');
    await expect(control).not.toBeFocused();
  });
}

test('a prompt appearing behind a modal does not take or defer focus and is not a modal restoration target', async ({
  page,
}) => {
  await page.goto(`${url}&method=input&delayed=true`);
  await trigger(page, 'Open Confirmation');
  const modal = page.getByRole('alertdialog');
  const cancel = modal.getByRole('button', { name: 'Cancel' });
  await expect(cancel).toBeFocused();
  await trigger(page, 'Toggle Disabled');
  await trigger(page, 'Show prompt');
  const prompt = page.locator('.extension-prompt');
  await expect(prompt).toBeVisible();
  await expect(cancel).toBeFocused();
  await cancel.click();
  await expect(modal).toBeHidden();
  await expect(
    page.getByRole('textbox', { name: 'Unrelated input' }),
  ).toBeFocused();
  await trigger(page, 'Toggle Disabled');
  await trigger(page, 'Update Prompt');
  await expect(
    page.getByRole('textbox', { name: 'Unrelated input' }),
  ).toBeFocused();
  await expect(prompt.getByRole('textbox')).not.toBeFocused();
});

test('a prompt neither focuses behind the palette nor steals focus on failure', async ({
  page,
}) => {
  await page.goto(`${url}&method=input&delayed=true`);
  await trigger(page, 'Open Palette');
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await expect(search).toBeFocused();
  await trigger(page, 'Show prompt');
  const input = page
    .locator('.extension-prompt')
    .getByRole('textbox', { includeHidden: true });
  await expect(input).toBeVisible();
  await expect(search).toBeFocused();
  await search.press('Escape');
  await expect(search).toBeHidden();
  await input.focus();
  await trigger(page, 'Begin Submit');
  await expect(input).toBeDisabled();
  await trigger(page, 'Open Palette');
  await expect(search).toBeFocused();
  await trigger(page, 'Fail Submit');
  await expect(input).toBeEnabled();
  await expect(search).toBeFocused();
});

test('confirmation failure repairs its disabled control without selecting a nonmodal prompt', async ({
  page,
}) => {
  await page.goto(`${url}&method=input`);
  await trigger(page, 'Open Confirmation');
  const modal = page.getByRole('alertdialog');
  const confirm = modal.getByRole('button', { name: 'Continue' });
  await confirm.focus();
  await expect(confirm).toBeFocused();
  await confirm.press('Enter');
  await expect(confirm).toBeDisabled();
  await trigger(page, 'Fail Confirmation');
  await expect(confirm).toBeFocused();
});
