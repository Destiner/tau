import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

async function renameShortcut(page: Page): Promise<void> {
  const mod = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform) ? 'Meta' : 'Control',
  );
  await page.keyboard.press(`${mod}+Shift+r`);
}

async function selection(
  input: Locator,
): Promise<[number | null, number | null]> {
  return input.evaluate((node: HTMLInputElement) => [
    node.selectionStart,
    node.selectionEnd,
  ]);
}

test('repeated rename command preserves an edited draft and selection', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await renameShortcut(page);
  const input = page.getByRole('textbox', { name: 'Session Name' });
  await input.fill('Unfinished session title');
  await input.evaluate((node: HTMLInputElement) =>
    node.setSelectionRange(3, 8, 'backward'),
  );
  await renameShortcut(page);
  await expect(input).toHaveValue('Unfinished session title');
  expect(await selection(input)).toEqual([3, 8]);
  await page.keyboard.press('Enter');
  await expect(page.locator('button.session-name')).toHaveText(
    'Unfinished session title',
  );
});
