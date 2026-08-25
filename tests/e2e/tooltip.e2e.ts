import type { Locator } from '@playwright/test';

import { expect, test } from './fixtures';

/**
 * Every icon-only control names itself on hover through one app-drawn tooltip
 * rather than the browser's native `title`.
 *
 * The two footer controls are the ones worth guarding: their button already
 * triggers a floating surface of its own, and a tooltip wrapped around such a
 * trigger silently takes the anchor its menu or popover positions itself from,
 * which lands that surface off-screen rather than failing outright.
 */
const scenarioUrl = '/?test-scenario=saved-session-bootstrap';

type Box = { x: number; y: number; width: number; height: number };

/*
 * Asserts a floating surface opened next to the control that opened it. The
 * trigger's own box has to be read before it opens: both of these mark the rest
 * of the page `aria-hidden` while they are up, which takes the button out of
 * reach of a role query.
 */
async function expectAnchoredTo(
  surface: Locator,
  triggerBox: Box | null,
): Promise<void> {
  const surfaceBox = await surface.boundingBox();
  expect(surfaceBox).not.toBeNull();
  expect(triggerBox).not.toBeNull();
  if (!surfaceBox || !triggerBox) return;
  expect(surfaceBox.y).toBeGreaterThan(0);
  expect(Math.abs(surfaceBox.x - triggerBox.x)).toBeLessThan(
    triggerBox.width + surfaceBox.width,
  );
  expect(
    Math.abs(surfaceBox.y + surfaceBox.height - triggerBox.y),
  ).toBeLessThan(60);
}

test('names icon-only controls on hover without native titles', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const openProject = page.getByRole('button', { name: 'Open Project' });
  await expect(openProject).not.toHaveAttribute('title', /./);
  await expect(openProject.locator('svg path')).toHaveAttribute(
    'd',
    'M216,72H131.31L104,44.69A15.86,15.86,0,0,0,92.69,40H40A16,16,0,0,0,24,56V200.62A15.4,15.4,0,0,0,39.38,216H216.89A15.13,15.13,0,0,0,232,200.89V88A16,16,0,0,0,216,72ZM92.69,56l16,16H40V56ZM216,200H40V88H216Zm-88-88a8,8,0,0,1,8,8v16h16a8,8,0,0,1,0,16H136v16a8,8,0,0,1-16,0V152H104a8,8,0,0,1,0-16h16V120A8,8,0,0,1,128,112Z',
  );

  await openProject.hover();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'Open Project' }),
  ).toBeVisible();

  // The trigger still triggers, and its menu still knows where the button is.
  const openProjectBox = await openProject.boundingBox();
  await openProject.click();
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  await expectAnchoredTo(menu, openProjectBox);
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);

  const reportIssue = page.getByRole('button', { name: 'Report an Issue' });
  await expect(reportIssue).not.toHaveAttribute('title', /./);
  await reportIssue.hover();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'Report an Issue' }),
  ).toBeVisible();
  const reportIssueBox = await reportIssue.boundingBox();
  await reportIssue.click();
  const popover = page.getByRole('dialog', { name: 'Report an Issue' });
  await expect(popover).toBeVisible();
  await expectAnchoredTo(popover, reportIssueBox);
  await page.keyboard.press('Escape');

  // An action that is invisible until its row is hovered still gets one.
  const projectRow = page.locator('.project-row').first();
  await projectRow.hover();
  const newSession = projectRow.getByRole('button', {
    name: /^New Session in/,
  });
  await newSession.hover();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'New Session' }),
  ).toBeVisible();

  // Moving off the trigger takes the tooltip with it.
  await page.getByRole('textbox', { name: 'Message Pi' }).hover();
  await expect(page.locator('.ui-tooltip')).toHaveCount(0);
});

test('shows the tooltip to a keyboard, not only to a pointer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  const newSession = page
    .getByRole('button', { name: 'New Session', exact: true })
    .first();
  await newSession.focus();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'New Session' }),
  ).toBeVisible();

  await newSession.blur();
  await expect(page.locator('.ui-tooltip')).toHaveCount(0);
});
