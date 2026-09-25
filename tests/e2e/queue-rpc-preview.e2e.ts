import { expect, test } from './fixtures';

test('uses the real composer and queue against a controllable mock Pi runtime', async ({
  page,
}) => {
  await page.goto('/?fixture=queue-rpc-preview');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Start a held preview');
  await composer.press('Enter');
  await expect(page.getByRole('button', { name: 'Advance Pi' })).toBeEnabled();
  await composer.fill('Steer while working');
  await expect(
    page.getByRole('button', { name: 'Queue Message' }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await composer.press('Enter');
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeVisible();
  await composer.fill('Follow up after work');
  await composer.press('Meta+Enter');
  const queue = page.getByRole('region', { name: 'Pending messages' });
  await expect(queue).toContainText('Steer while working');
  await expect(queue).toContainText('Follow up after work');
  await expect(queue.locator(':scope > button').first()).toHaveAttribute(
    'aria-label',
    'Clear All',
  );
  await expect(queue.locator('.queue-divider')).toHaveCount(1);
  expect(
    await queue.evaluate(
      (element) =>
        element.querySelector('.queue-rail')!.getBoundingClientRect().width <
        element.getBoundingClientRect().width - 40,
    ),
  ).toBe(true);
  await expect(queue.locator('.queue-ordinal')).toHaveText('1');
  await expect(
    queue.locator('.queue-chip').nth(1).locator(':scope > span').first(),
  ).toHaveClass('queue-ordinal');
  await expect(queue.locator('.queue-ordinal')).toHaveCSS(
    'border-radius',
    '4px',
  );
  const firstChip = queue.locator('.queue-chip').first();
  await expect(firstChip).not.toHaveAttribute('title');
  await firstChip.hover();
  await expect(page.getByRole('tooltip')).toContainText('Steer while working');
  expect(
    await page
      .getByRole('tooltip')
      .evaluate((element) => element.getBoundingClientRect().width),
  ).toBeLessThan(300);
  await expect(page.getByRole('tooltip')).not.toContainText(
    'Follow up after work',
  );
  await queue.locator('.queue-chip').nth(1).hover();
  await expect(page.getByRole('tooltip')).toContainText('Follow Up • 1');
  await expect(page.getByRole('tooltip')).toContainText('Follow up after work');
  await page.getByRole('button', { name: 'Advance Pi' }).hover();
  await page.waitForTimeout(110);
  await expect(page.getByRole('tooltip')).toHaveCount(0);
  await page.getByRole('button', { name: 'Advance Pi' }).click();
  await expect(page.getByRole('region', { name: 'Transcript' })).toContainText(
    'Steer while working',
  );
  await expect(queue).toContainText('Follow up after work');
  await page.getByRole('button', { name: 'Advance Pi' }).click();
  await expect(page.getByRole('region', { name: 'Transcript' })).toContainText(
    'Follow up after work',
  );
  await expect(queue).toHaveCount(0);
});

test('keeps a long queued markdown preview inside a short window', async ({
  page,
}) => {
  await page.setViewportSize({ width: 560, height: 320 });
  await page.goto('/?fixture=queue-rpc-preview');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Start held work');
  await composer.press('Enter');
  await expect(page.getByRole('button', { name: 'Advance Pi' })).toBeEnabled();
  await composer.fill(
    'Please check the narrow layout and preserve this long message. '.repeat(
      12,
    ),
  );
  await composer.press('Enter');
  await page.locator('.queue-chip').first().hover();
  const preview = page.getByRole('tooltip');
  await expect(preview).toContainText('Please check the narrow layout');
  const bounds = await preview.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(320);
  expect(bounds!.width).toBeLessThanOrEqual(500);
});
