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

test('describes the focused session title without replacing its other description', async ({
  page,
}) => {
  await page.goto('/');
  const title = page.getByRole('heading', { level: 1 }).getByRole('button', {
    name: 'Workspace overview',
    exact: true,
  });
  const hint = page.getByRole('tooltip').filter({ hasText: 'Rename Session' });
  await title.evaluate((button) => {
    const description = document.createElement('span');
    description.id = 'test-title-description';
    description.hidden = true;
    description.textContent = 'Current session.';
    document.body.append(description);
    button.setAttribute('aria-describedby', description.id);
  });

  await title.focus();
  await expect(hint).toBeVisible();
  await expect(title).toHaveAccessibleDescription(
    /^Current session\. Rename Session (?:⌘⇧R|Ctrl\+Shift\+R)$/,
  );
  const hintId = await hint.getAttribute('id');
  expect(hintId).toBeTruthy();

  await title.press('Escape');
  await expect(hint).toHaveCount(0);
  await expect(title).toBeFocused();
  await expect(title).toHaveAttribute(
    'aria-describedby',
    'test-title-description',
  );
  await expect(title).toHaveAccessibleDescription('Current session.');

  await title.blur();
  await title.focus();
  await expect(hint).toHaveAttribute('id', hintId!);
});

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
