import { expect, test } from './fixtures';

/* The panel is portaled next to the overlay rather than inside it, so it
 * cannot inherit the overlay's centering: a regression drops it into normal
 * flow at the top-left of the page. */

const viewport = { width: 900, height: 600 };

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(viewport);
});

for (const step of ['connection'] as const) {
  test(`anchors the ${step} panel top-center, clear of the title bar`, async ({
    page,
  }) => {
    await page.goto(`/?fixture=remote-dialog&step=${step}`);

    const panel = page.getByRole('dialog');
    await expect(panel).toBeVisible();

    const box = await panel.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;

    // The native title bar occupies the first 30px; stay in the upper third.
    expect(box.y).toBeGreaterThanOrEqual(30);
    expect(box.y).toBeLessThan(viewport.height / 3);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
    expect(box.x + box.width / 2).toBeCloseTo(viewport.width / 2, 0);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  });
}

for (const step of ['connection'] as const) {
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

test('remote directories remain keyboard and wheel reachable at native minimum size', async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 520 });
  await page.goto('/?fixture=remote-dialog&step=directory');

  const dialog = page.getByRole('dialog');
  const filter = dialog.getByRole('textbox', {
    name: 'Filter Remote Directories',
  });
  const options = dialog.getByRole('option');
  await expect(filter).toBeFocused();
  await expect(options).toHaveCount(24);
  for (let index = 1; index < 24; index += 1) {
    await filter.press('ArrowDown');
  }
  await expect(options.last()).toHaveAttribute('aria-selected', 'true');
  await expect(options.last()).toBeInViewport({ ratio: 1 });
  await expect(filter).toBeFocused();
  await expect(filter).toBeInViewport({ ratio: 1 });

  const list = dialog.getByRole('listbox');
  const box = await list.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.wheel(0, -10000);
  await expect(options.first()).toBeInViewport({ ratio: 1 });
  await expect(options.last()).not.toBeInViewport();
  await expect(filter).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('cannot be dismissed while a connection is pending', async ({ page }) => {
  await page.goto('/?fixture=remote-dialog&connecting=true');

  const panel = page.getByRole('dialog');
  await expect(panel).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(panel).toBeVisible();
  await page.locator('.ui-dialog-overlay').click({ position: { x: 5, y: 5 } });
  await expect(panel).toBeVisible();
});

for (const mode of ['local', 'remote'] as const) {
  test(`opens an existing ${mode} project without losing the source draft`, async ({
    page,
  }) => {
    await page.goto(`/?test-scenario=project-opening&project-opening=${mode}`);

    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await expect(composer).toBeEnabled();
    await composer.fill('Keep the source draft');

    await page.getByRole('button', { name: 'Open Project' }).click();
    await page
      .getByRole('menuitem', {
        name: mode === 'local' ? 'Open Local Project' : 'Open Remote Project',
      })
      .click();

    if (mode === 'remote') {
      const connection = page.getByRole('textbox', {
        name: 'SSH Connection String',
      });
      await connection.fill('ssh target@example');
      await connection.press('Meta+Enter');
      await page
        .getByRole('option', { name: 'Select /remote/target-project' })
        .click();
    }

    const targetSession = page.locator('[data-session-id="session-target"]');
    await expect(targetSession).toHaveClass(/selected/);
    await expect(composer).toBeEnabled();
    await expect(composer).toHaveValue('');
    await composer.fill('Target composer is usable');

    await page
      .locator('[data-session-id="session-main"] .session-select')
      .click();
    await expect(composer).toHaveValue('Keep the source draft');
  });
}

test('restores focus to the open-project trigger after dismissal', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-bootstrap');

  const trigger = page.getByRole('button', { name: 'Open Project' });
  await trigger.click();
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();

  const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');

  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
});
