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

test('renders a table in the question as a table', async ({ page }) => {
  const dialog = page.getByRole('dialog');
  const header = dialog.locator('.extension-dialog-message th').first();
  await expect(header).toBeVisible();

  const style = await header.evaluate((element) => {
    const computed = window.getComputedStyle(element);
    return {
      borderWidth: Number.parseFloat(computed.borderTopWidth),
      padding: Number.parseFloat(computed.paddingLeft),
      background: computed.backgroundColor,
      textAlign: computed.textAlign,
    };
  });
  expect(style.borderWidth).toBeGreaterThan(0);
  expect(style.padding).toBeGreaterThan(0);
  expect(style.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(style.textAlign).toBe('left');
});

test('scrolls a table wider than the question inside itself', async ({
  page,
}) => {
  const dialog = page.getByRole('dialog');
  const table = dialog.locator('.extension-dialog-message table');
  const body = dialog.locator('.extension-dialog-body');

  const width = await table.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(width.scrollWidth).toBeGreaterThan(width.clientWidth);

  // The table takes the sideways scrolling, so the prompt keeps its own width.
  const bodyWidth = await body.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(bodyWidth.scrollWidth).toBe(bodyWidth.clientWidth);
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
