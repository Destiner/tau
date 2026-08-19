import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=extension-dialog';

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto(fixtureUrl);
});

test('keeps a question larger than the pane inside the composer', async ({
  page,
}) => {
  const dialog = page.getByRole('dialog');
  const actions = dialog.getByRole('button', { name: 'Cancel' });
  await expect(actions).toBeVisible();

  const viewport = page.viewportSize();
  const composerBox = await dialog.boundingBox();
  const actionsBox = await actions.boundingBox();
  expect(composerBox).not.toBeNull();
  expect(actionsBox).not.toBeNull();
  if (!composerBox || !actionsBox || !viewport) return;

  // The block bounds its own height, and the way out stays on the screen.
  expect(composerBox.height).toBeLessThanOrEqual(viewport.height);
  expect(actionsBox.y + actionsBox.height).toBeLessThanOrEqual(viewport.height);
  expect(actionsBox.y + actionsBox.height).toBeLessThanOrEqual(
    composerBox.y + composerBox.height + 1,
  );
});

test('scrolls the question and its options as one region', async ({ page }) => {
  const dialog = page.getByRole('dialog');
  const body = dialog.locator('.extension-dialog-body');
  const options = dialog.getByRole('listbox');

  const overflow = await body.evaluate((element) => ({
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
    scrollTop: element.scrollTop,
  }));
  expect(overflow.scrollHeight).toBeGreaterThan(overflow.clientHeight);
  // The question is what the reader sees first, not the options below it.
  expect(overflow.scrollTop).toBe(0);

  // The options ride along with the question rather than scrolling on their own.
  const optionsScroll = await options.evaluate((element) => ({
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  expect(optionsScroll.scrollHeight).toBe(optionsScroll.clientHeight);
});

test('reaches an option below the fold and reports the choice', async ({
  page,
}) => {
  const dialog = page.getByRole('dialog');
  const option = dialog.getByRole('option', { name: 'label-11' });

  await option.scrollIntoViewIfNeeded();
  await option.click();

  await expect(page.getByTestId('dialog-outcome')).toHaveText(
    '{"submit":"label-11"}',
  );
});
