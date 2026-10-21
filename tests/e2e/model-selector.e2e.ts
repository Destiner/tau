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

test('keeps current and traversal states distinct and resets on dismissal', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  const trigger = page.getByRole('combobox', { name: 'Model', exact: true });
  await trigger.click();
  const search = page.getByRole('searchbox', { name: 'Search Models' });
  const current = page.locator('.ui-select-option[data-current]');
  const cursor = page.locator('.ui-select-option[data-cursor]');
  await expect(current).toHaveText('GPT-5.6 Sol');
  await expect(cursor).toHaveText('GPT-5.6 Sol');

  await search.press('ArrowDown');
  await expect(cursor).toHaveText('GPT-6 Astra');
  await page.getByRole('option', { name: 'Claude Opus 5' }).hover();
  await expect(cursor).toHaveText('Claude Opus 5');
  await search.fill('missing');
  await expect(page.getByText('No matching models')).toBeVisible();
  await search.fill('');
  await expect(cursor).toHaveText('Claude Opus 5');

  await search.fill('claude');
  await page.getByRole('button', { name: 'Outside' }).click();
  await expect(search).toHaveCount(0);
  await trigger.click();
  await expect(search).toHaveValue('');
});

test('omits search chrome when no models are available', async ({ page }) => {
  await page.goto(fixtureUrl);

  const trigger = page.getByRole('combobox', { name: 'Empty Model' });
  await expect(trigger).toHaveText('No Models');
  await trigger.click();

  await expect(page.getByText('No models available')).toBeVisible();
  await expect(
    page.getByRole('searchbox', { name: 'Search Models' }),
  ).toHaveCount(0);
});
