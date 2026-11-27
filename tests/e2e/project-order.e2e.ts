import { expect, test } from './fixtures';

test('shows the resize hint without intercepting pointer capture or arrow keys', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  const handle = page.getByRole('separator', { name: 'Resize Sidebar' });
  const tooltip = page.locator('.ui-tooltip:visible');
  await handle.hover();
  await expect(tooltip).toHaveCount(1);
  await expect(tooltip).toHaveText('Resize Sidebar');
  const initialWidth = Number(await handle.getAttribute('aria-valuenow'));
  const box = (await handle.boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.down();
  await expect(tooltip).toHaveCount(0);
  await page.mouse.move(x + 25, y, { steps: 4 });
  await page.mouse.up();
  const resizedWidth = Number(await handle.getAttribute('aria-valuenow'));
  expect(resizedWidth).toBeGreaterThan(initialWidth + 15);
  await handle.press('ArrowLeft');
  await expect(handle).toHaveAttribute(
    'aria-valuenow',
    String(resizedWidth - 10),
  );
  await expect(handle).toBeFocused();
  await expect(composer).toBeEnabled();
});

test('reorders projects and saves the order before and after visiting the archive', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const groups = page.locator('.projects-body > .project-group');
  const atlas = '/browser-dev/projects/atlas';
  const notes = '/browser-dev/projects/notes';

  for (const returningFromArchive of [false, true]) {
    if (returningFromArchive) {
      await page
        .getByRole('button', { name: 'Show Archived Sessions' })
        .click();
      await expect(page.locator('.archived-list')).toBeVisible();
      await page.getByRole('button', { name: 'Show Sessions' }).click();
    }

    const handle = await groups
      .first()
      .locator('.project-drag-handle')
      .boundingBox();
    const target = await groups.last().boundingBox();
    if (!handle || !target) throw new Error('Expected project drag targets.');
    const x = handle.x + handle.width / 2;
    const y = handle.y + handle.height / 2;
    await page.mouse.move(x, y);
    const tooltip = page.locator('.ui-tooltip:visible');
    await expect(tooltip).toContainText('Drag to Reorder');
    await page.mouse.down();
    await page.mouse.move(x, y + 8, { steps: 4 });
    await expect(page.locator('body > .project-sortable-drag')).toBeVisible();
    await page.mouse.move(x, target.y + target.height - 5, { steps: 20 });

    const expected = returningFromArchive ? [atlas, notes] : [notes, atlas];
    const visibleOrder = (): Promise<(string | null)[]> =>
      groups.evaluateAll((elements) =>
        elements.map((element) => element.getAttribute('data-project-path')),
      );
    await expect.poll(visibleOrder).toEqual(expected);
    await page.mouse.up();
    await expect(page.locator('body > .project-sortable-drag')).toHaveCount(0);
    await expect.poll(visibleOrder).toEqual(expected);
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const workspace = (await window.__TAURI_INTERNALS__?.invoke(
            'load_workspace',
          )) as { projects: { path: string }[] };
          return workspace.projects.map((project) => project.path);
        }),
      )
      .toEqual(expected);
  }
});
