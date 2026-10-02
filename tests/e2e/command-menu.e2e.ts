import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=command-list-scrolling';

async function nextFrame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
  );
}

async function scrollTop(page: Page): Promise<number> {
  return page.locator('.command-menu').evaluate((menu) => menu.scrollTop);
}

async function partiallyClipSecondCommand(page: Page): Promise<void> {
  await page.locator('.command-menu').evaluate((menu) => {
    const row = menu.querySelector<HTMLElement>('.command-option:nth-child(2)');
    if (!row) throw new Error('Expected a second slash command.');
    menu.scrollTop +=
      row.getBoundingClientRect().top - menu.getBoundingClientRect().top + 8;
  });
}

test('slash-command hover highlights a clipped row without moving its native scroll position', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('/');
  const menu = page.locator('.command-menu');
  await expect(menu).toBeVisible();
  expect(
    await menu.evaluate(
      (element) => element.scrollHeight > element.clientHeight,
    ),
  ).toBe(true);

  await nextFrame(page);
  await page.mouse.move(0, 0);
  await partiallyClipSecondCommand(page);
  const before = await scrollTop(page);
  const row = menu.locator('.command-option:nth-child(2)');
  const rowBounds = await row.boundingBox();
  const menuBounds = await menu.boundingBox();
  expect(rowBounds).not.toBeNull();
  expect(menuBounds).not.toBeNull();
  const visibleTop = Math.max(menuBounds!.y, rowBounds!.y);
  const visibleBottom = Math.min(
    menuBounds!.y + menuBounds!.height,
    rowBounds!.y + rowBounds!.height,
  );
  expect(visibleBottom).toBeGreaterThan(visibleTop);
  await page.mouse.move(rowBounds!.x + 12, (visibleTop + visibleBottom) / 2);
  await nextFrame(page);

  await expect(row).toHaveAttribute('aria-selected', 'true');
  expect(Math.abs((await scrollTop(page)) - before)).toBeLessThanOrEqual(1);
  await expect(composer).toBeFocused();
});

test('slash-command wheel scrolling stays native at both pointer edges', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('/');
  const menu = page.locator('.command-menu');
  await expect(menu).toBeVisible();

  await menu.evaluate((element) => {
    element.scrollTop = 80;
  });
  const bounds = await menu.boundingBox();
  expect(bounds).not.toBeNull();
  await page.mouse.move(bounds!.x + 20, bounds!.y + bounds!.height - 3);
  const beforeDown = await scrollTop(page);
  await page.mouse.wheel(0, 13);
  await nextFrame(page);
  const afterDown = await scrollTop(page);
  expect(afterDown).toBeGreaterThan(beforeDown);
  expect(afterDown - beforeDown).toBeLessThanOrEqual(20);

  await page.mouse.move(bounds!.x + 20, bounds!.y + 3);
  await nextFrame(page);
  const beforeUp = await scrollTop(page);
  await page.mouse.wheel(0, -13);
  await nextFrame(page);
  const afterUp = await scrollTop(page);
  expect(afterUp).toBeLessThan(beforeUp);
  expect(beforeUp - afterUp).toBeLessThanOrEqual(20);
  await expect(composer).toBeFocused();
});

test('slash-command endpoints do not stick after outward wheel input', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('/');
  const menu = page.locator('.command-menu');
  await expect(menu).toBeVisible();
  const bounds = await menu.boundingBox();
  expect(bounds).not.toBeNull();

  await page.mouse.move(bounds!.x + 20, bounds!.y + bounds!.height - 3);
  await menu.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  const bottom = await scrollTop(page);
  await page.mouse.wheel(0, 24);
  await nextFrame(page);
  expect(await scrollTop(page)).toBe(bottom);
  await page.mouse.wheel(0, -13);
  await nextFrame(page);
  expect(await scrollTop(page)).toBeLessThan(bottom);

  await page.mouse.move(bounds!.x + 20, bounds!.y + 3);
  await menu.evaluate((element) => {
    element.scrollTop = 0;
  });
  await page.mouse.wheel(0, -24);
  await nextFrame(page);
  expect(await scrollTop(page)).toBe(0);
  await page.mouse.wheel(0, 13);
  await nextFrame(page);
  expect(await scrollTop(page)).toBeGreaterThan(0);
  await expect(composer).toBeFocused();
});

test('slash-command query resets reveal the first command after manual scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('/');
  const menu = page.locator('.command-menu');
  await expect(menu).toBeVisible();
  await menu.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });

  await composer.fill('/scroll-command-17');
  await expect(menu.locator('.command-option')).toHaveCount(1);
  await expect(menu.locator('.command-option')).toHaveAttribute(
    'aria-selected',
    'true',
  );
  await composer.fill('/');
  const first = menu.locator('.command-option:nth-child(1)');
  await expect(first).toHaveAttribute('aria-selected', 'true');
  expect(
    await first.evaluate((row) => {
      const list = row.parentElement!;
      const rowBounds = row.getBoundingClientRect();
      const listBounds = list.getBoundingClientRect();
      return (
        rowBounds.top >= listBounds.top && rowBounds.bottom <= listBounds.bottom
      );
    }),
  ).toBe(true);
  await expect(composer).toBeFocused();
});

test('slash-command keyboard navigation reveals its selection after manual scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('/');
  const menu = page.locator('.command-menu');
  await expect(menu).toBeVisible();
  await menu.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });

  // ArrowUp clamps at the first command. It must still deliberately reveal
  // that unchanged selection after a user has scrolled it out of view.
  await composer.press('ArrowUp');
  const selected = menu.locator('.command-option:nth-child(1)');
  await expect(selected).toHaveAttribute('aria-selected', 'true');
  await expect(composer).toHaveAttribute(
    'aria-activedescendant',
    'command-option-0',
  );
  await expect(selected).toBeInViewport();
  await expect(composer).toBeFocused();
});
