import { expect, test } from './fixtures';

const sandboxUrl = '/';

test('starts the browser sandbox with two projects and five sessions', async ({
  page,
}) => {
  await page.goto(sandboxUrl);

  await expect(page.locator('.project-group')).toHaveCount(2);
  await expect(page.locator('.session-row')).toHaveCount(5);
  await expect(
    page
      .locator('.session-row', { hasText: 'Workspace overview' })
      .locator('.session-select'),
  ).toHaveAttribute('aria-current', 'page');
  await expect(
    page.getByText('Give me a quick overview of the Atlas workspace.'),
  ).toBeVisible();
});

test('keeps sandbox sidebar widths isolated from browser storage and tabs', async ({
  context,
  page,
}) => {
  await context.addInitScript(() => {
    localStorage.setItem('unrelated.preference', 'keep-me');
    localStorage.setItem('tau.sidebar-width', '333');
  });
  await page.goto(sandboxUrl);

  const firstHandle = page.getByRole('separator', { name: 'Resize Sidebar' });
  await expect(firstHandle).toHaveAttribute('aria-valuenow', '260');
  await firstHandle.press('End');
  await expect(firstHandle).toHaveAttribute('aria-valuenow', '480');

  const secondPage = await context.newPage();
  await secondPage.goto(sandboxUrl);
  const secondHandle = secondPage.getByRole('separator', {
    name: 'Resize Sidebar',
  });
  await expect(secondHandle).toHaveAttribute('aria-valuenow', '260');
  await secondHandle.press('ArrowRight');
  await expect(secondHandle).toHaveAttribute('aria-valuenow', '270');
  await expect(firstHandle).toHaveAttribute('aria-valuenow', '480');

  await expect
    .poll(() =>
      page.evaluate(() => ({
        unrelated: localStorage.getItem('unrelated.preference'),
        sidebar: localStorage.getItem('tau.sidebar-width'),
      })),
    )
    .toEqual({ unrelated: 'keep-me', sidebar: '333' });
  await secondPage.close();
});

test('replies locally and restores the browser sandbox seed on reload', async ({
  page,
}) => {
  const prompt = 'Confirm browser sandbox reset';

  await page.goto(sandboxUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.fill(prompt);
  await page.getByRole('button', { name: 'Send Message' }).click();

  await expect(
    page.getByText(`Browser sandbox received: “${prompt}”`),
  ).toBeVisible();

  await page.reload();

  await expect(page.locator('.project-group')).toHaveCount(2);
  await expect(page.locator('.session-row')).toHaveCount(5);
  await expect(page.getByText(prompt)).toHaveCount(0);
  await expect(
    page.getByText('Give me a quick overview of the Atlas workspace.'),
  ).toBeVisible();
});
