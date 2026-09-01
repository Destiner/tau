import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-order';

test('keeps project dragging after returning from archived sessions', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  await expect(page.locator('.archived-list')).toBeVisible();
  await page.getByRole('button', { name: 'Show Sessions' }).click();

  const project = page.locator('.project-group').first();
  const handle = project.locator('.project-drag-handle');
  const box = await handle.boundingBox();
  expect(box).not.toBeNull();
  if (!box) return;

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 + 8);
  await expect(project).toHaveClass(/project-sortable-chosen/);
  await page.mouse.up();
});
