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
