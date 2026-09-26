import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/?fixture=archive');
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
});

test('renders bounded batches and reaches older groups without losing focus', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const list = page.locator('.archived-list');
  const rows = list.locator('.row');
  await expect(rows).toHaveCount(50);
  await page.waitForTimeout(150);
  await expect(rows).toHaveCount(50);
  await expect(rows.first()).toContainText('Archive 0');
  await expect(list.getByText('Yesterday', { exact: true })).toHaveCount(0);

  await rows.first().locator('.copy').focus();
  await list.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(rows).toHaveCount(100);
  await expect(rows.first().locator('.copy')).toBeFocused();
  await expect(rows.last()).toContainText('Archive 99');
  await expect(rows).toHaveCount(100);

  await list.getByRole('button', { name: 'Today' }).click();
  await expect(rows).toHaveCount(100);
  await expect(list.getByText('Yesterday', { exact: true })).toBeVisible();
  await list.getByRole('button', { name: 'Today' }).click();
  await expect(rows).toHaveCount(100);

  // Navigate to the end a batch at a time, checking stable identities.
  for (let i = 0; i < 50; i++) {
    const count = await rows.count();
    if (count === 2500) break;
    await list.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(rows).toHaveCount(Math.min(count + 50, 2500));
  }
  await expect(rows).toHaveCount(2500);
  await expect(rows.last()).toContainText('Archive 2499');
  const titles = await rows.locator('.name').allTextContents();
  expect(new Set(titles).size).toBe(2500);

  await page.getByRole('button', { name: 'Show Sessions' }).click();
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  await expect(page.locator('.archived-list .row')).toHaveCount(50);
});

test('keyboard scrolling appends without changing the focused control', async ({
  page,
}) => {
  const list = page.locator('.archived-list');
  const rows = list.locator('.row');
  const first = rows.first().locator('.copy');
  await first.focus();
  await page.keyboard.press('End');
  await expect(rows).toHaveCount(100);
  await expect(first).toBeFocused();
});

test('small and boundary archives never mount nonexistent rows', async ({
  page,
}) => {
  const rows = page.locator('.archived-list .row');
  for (const count of [1, 49, 50, 51]) {
    await page.evaluate(
      (size) => window.__TAU_ARCHIVE_FIXTURE__!.replace(size),
      count,
    );
    await expect(rows).toHaveCount(Math.min(count, 50));
  }
});

test('collapsing and refreshing exposes eligible rows, empty state is truthful', async ({
  page,
}) => {
  const list = page.locator('.archived-list');
  const rows = list.locator('.row');
  await list.getByRole('button', { name: 'Today' }).click();
  await expect(rows).toHaveCount(50);
  await expect(rows.first()).toContainText('Archive 120');
  await page.evaluate(() =>
    window.__TAU_ARCHIVE_FIXTURE__!.remove('/fixture/project-0', 'archive-120'),
  );
  await expect(rows.first()).toContainText('Archive 121');
  await page.evaluate(() => window.__TAU_ARCHIVE_FIXTURE__!.replace(0));
  await expect(list.getByText('No archived sessions')).toBeVisible();
  await expect(rows).toHaveCount(0);
});
