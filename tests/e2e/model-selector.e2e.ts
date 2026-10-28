import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=model-selector';

test('groups, filters, and traverses models in rendered order', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  const trigger = page.getByRole('combobox', { name: 'Model', exact: true });
  await expect(trigger).toHaveText('GPT-5.6 Sol');
  await trigger.click();

  const search = page.getByRole('searchbox', { name: 'Search Models' });
  await expect(search).toBeFocused();
  await expect(page.getByText('OpenAI Codex', { exact: true })).toBeVisible();
  await expect(page.getByText('OpenRouter', { exact: true })).toBeVisible();
  await expect(page.getByText('Pi Claude', { exact: true })).toBeVisible();
  await expect(page.getByRole('option').allTextContents()).resolves.toEqual([
    'GPT-5.6 Sol',
    'GPT-6 Astra',
    'DeepSeek V4 Flash',
    'Claude Fable 5',
    'Claude Opus 5',
  ]);

  await expect(page.locator('.ui-select-option[data-cursor]')).toHaveText(
    'GPT-5.6 Sol',
  );
  await search.press('ArrowDown');
  await expect(page.locator('.ui-select-option[data-cursor]')).toHaveText(
    'GPT-6 Astra',
  );

  await search.fill('openai-codex');
  await expect(page.getByText('No matching models')).toBeVisible();
  await search.fill('astra');
  await expect(page.getByRole('option')).toHaveText('GPT-6 Astra');
  await search.press('Enter');

  await expect(trigger).toHaveText('GPT-6 Astra');
  await expect(search).toHaveCount(0);
  await trigger.click();
  await expect(search).toHaveValue('');
  await expect(page.getByRole('option')).toHaveCount(5);
});
