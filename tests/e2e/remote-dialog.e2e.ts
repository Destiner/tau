import { expect, test } from './fixtures';

/* The panel is portaled next to the overlay rather than inside it, so it
 * cannot inherit the overlay's centering: a regression drops it into normal
 * flow at the top-left of the page. */

const viewport = { width: 900, height: 600 };

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(viewport);
});

for (const step of ['connection', 'directory'] as const) {
  test(`anchors the ${step} panel top-center, clear of the title bar`, async ({
    page,
  }) => {
    await page.goto(`/?fixture=remote-dialog&step=${step}`);

    const panel = page.getByRole('dialog');
    await expect(panel).toBeVisible();

    const box = await panel.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;

    expect(box.y).toBeCloseTo(68, 0);
    expect(box.x + box.width / 2).toBeCloseTo(viewport.width / 2, 0);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  });
}

for (const step of ['connection', 'directory'] as const) {
  test(`renders the ${step} error below its input`, async ({ page }) => {
    await page.goto(`/?fixture=remote-dialog&step=${step}&error=true`);

    const input = page.getByRole('textbox');
    const error = page.getByRole('alert');
    await expect(error).toHaveText(
      'The remote connection failed. Check the connection and try again.',
    );
    const errorId = await error.getAttribute('id');
    expect(errorId).not.toBeNull();
    await expect(input).toHaveAttribute('aria-describedby', errorId ?? '');
    expect(await input.getAttribute('title')).toBeNull();

    const inputBox = await input.boundingBox();
    const errorBox = await error.boundingBox();
    expect(inputBox).not.toBeNull();
    expect(errorBox).not.toBeNull();
    if (inputBox && errorBox) {
      expect(errorBox.y).toBeGreaterThanOrEqual(inputBox.y + inputBox.height);
    }
  });
}

test('cannot be dismissed while a connection is pending', async ({ page }) => {
  await page.goto('/?fixture=remote-dialog&connecting=true');

  const panel = page.getByRole('dialog');
  await expect(panel).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(panel).toBeVisible();
  await page.locator('.ui-dialog-overlay').click({ position: { x: 5, y: 5 } });
  await expect(panel).toBeVisible();
});

test('keeps a margin from the window edges when it is narrower than the panel', async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 600 });
  await page.goto('/?fixture=remote-dialog');

  const box = await page.getByRole('dialog').boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;

  expect(box.x).toBeGreaterThanOrEqual(12);
  expect(box.x + box.width).toBeLessThanOrEqual(320 - 12);
});
