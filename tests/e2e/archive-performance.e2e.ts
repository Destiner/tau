import { expect, test } from '@playwright/test';

test('opens a 2500-session archive without mounting the whole list', async ({
  page,
}, testInfo) => {
  await page.goto('/?fixture=archive');
  await page.evaluate(() => document.fonts.ready);
  const metrics: { open: number; append: number }[] = [];
  for (let i = 0; i < 3; i++) {
    const open = await page.evaluate(async () => {
      const button = document.querySelector<HTMLButtonElement>(
        '[aria-label="Show Archived Sessions"]',
      )!;
      const start = performance.now();
      button.click();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
      return performance.now() - start;
    });
    const list = page.locator('.archived-list');
    await expect(list.locator('.row')).toHaveCount(50);
    const start = Date.now();
    await list.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(list.locator('.row')).toHaveCount(100);
    const append = Date.now() - start;
    metrics.push({ open, append });
    await page.getByRole('button', { name: 'Show Sessions' }).click();
  }
  await testInfo.attach('archive-performance-metrics', {
    body: JSON.stringify(metrics, null, 2),
    contentType: 'application/json',
  });
  expect(
    metrics.map((sample) => sample.open).sort((a, b) => a - b)[1],
  ).toBeLessThan(450);
  expect(
    metrics.map((sample) => sample.append).sort((a, b) => a - b)[1],
  ).toBeLessThan(450);
});
