import type { Locator } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-tooltip';
const longTitle =
  '<strong>Do not render markup</strong> — this intentionally long session title is clipped in the sidebar';
const archivedTitle =
  '<em>Archived markup stays text</em> — a long archived session title';

async function expectBottomStartAlignment(
  trigger: Locator,
  tooltip: Locator,
): Promise<void> {
  const [triggerBox, tooltipBox] = await Promise.all([
    trigger.boundingBox(),
    tooltip.boundingBox(),
  ]);
  expect(triggerBox).not.toBeNull();
  expect(tooltipBox).not.toBeNull();
  if (!triggerBox || !tooltipBox) return;

  // A small tolerance permits device-pixel rounding while still catching a
  // centered tooltip or one anchored to the wrong element.
  expect(Math.abs(tooltipBox.x - triggerBox.x)).toBeLessThan(12);
  expect(tooltipBox.y).toBeGreaterThanOrEqual(
    triggerBox.y + triggerBox.height - 1,
  );
}

test('shows a complete, app-drawn tooltip for an ellipsized session title', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  const row = page.locator('.session-row', { hasText: longTitle });
  const trigger = row.locator('.session-select');
  const title = row.locator('.session-title');
  await expect(title).toHaveText(longTitle);
  await expect(title).toHaveCSS('pointer-events', 'none');
  const isEllipsized = await title.evaluate(
    (element) => element.scrollWidth > element.clientWidth,
  );
  expect(isEllipsized).toBe(true);
  expect(
    await title.evaluate((element) => {
      const box = element.getBoundingClientRect();
      return (
        document.elementFromPoint(box.left + 8, box.top + box.height / 2) ===
        element
      );
    }),
  ).toBe(false);

  // The clipped copy deliberately does not receive pointer input in WebKit:
  // hovering it must still reach the button that owns the app tooltip.
  await title.hover();
  const tooltip = page.locator('.ui-tooltip', { hasText: longTitle });
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Working',
  );
  await expect(tooltip.locator('.session-tooltip-name')).toHaveText(longTitle);
  await expect(tooltip.locator('.session-tooltip-date')).toHaveText(
    '2 hours ago',
  );
  await expect(tooltip).not.toContainText('working-session-opaque-7fb4d9');
  await expect(tooltip.locator('strong')).toHaveCount(0);
  await expectBottomStartAlignment(trigger, tooltip);

  // Focus is an interaction path too, and does not expose a browser title.
  await expect(trigger).not.toHaveAttribute('title', /./);
  await page.getByTestId('outside-sidebar').hover();
  await trigger.focus();
  await expect(tooltip).toBeVisible();
});

test('keeps tooltip contents reactive without exposing fixture session IDs', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  const replacementTitle = '<mark>Updated markup remains text</mark>';
  await page.evaluate((title) => {
    const fixture = window.__TAU_SESSION_TOOLTIP_FIXTURE__;
    if (!fixture) throw new Error('Expected session-tooltip fixture API.');
    fixture.setStatus('draft');
    fixture.setTitle(title);
  }, replacementTitle);

  const row = page.locator('.session-row', { hasText: replacementTitle });
  await row.locator('.session-select').hover();
  const tooltip = page.locator('.ui-tooltip', { hasText: replacementTitle });
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Unsent draft',
  );
  await expect(tooltip.locator('.session-tooltip-name')).toHaveText(
    replacementTitle,
  );
  await expect(tooltip).not.toContainText('working-session-opaque-7fb4d9');
  await expect(tooltip.locator('mark')).toHaveCount(0);

  await page.evaluate(() => {
    window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setStatus('unread');
    window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setTitle('Changed while open');
  });
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText('Unread');
  await expect(tooltip.locator('.session-tooltip-name')).toHaveText(
    'Changed while open',
  );
  await expect(page.locator('.session-row').nth(1)).toContainText(
    'Prepare release notes',
  );

  const unbroken = `会話${'長'.repeat(120)}🚀`;
  await page.evaluate(
    (title) => window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setTitle(title),
    unbroken,
  );
  await expect(tooltip.locator('.session-tooltip-name')).toHaveText(unbroken);
  const bounds = await tooltip.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(
    page.viewportSize()!.width,
  );

  await page.getByTestId('outside-sidebar').hover();
  await expect(tooltip).toBeHidden();
});

test('uses the same structured tooltip while reviewing archived sessions', async ({
  page,
}) => {
  await page.goto(fixtureUrl);

  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  const archivedList = page.locator('.archived-list');
  await expect(archivedList).toBeVisible();
  await expect(page.locator('.session-row')).toHaveCount(0);

  const row = archivedList.locator('.row', { hasText: archivedTitle });
  const trigger = row.locator('.copy');
  await trigger.hover();
  const tooltip = page.locator('.ui-tooltip', { hasText: archivedTitle });
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Archived',
  );
  await expect(tooltip.locator('.session-tooltip-name')).toHaveText(
    archivedTitle,
  );
  await expect(tooltip.locator('.session-tooltip-date')).toHaveText(
    '3 weeks ago',
  );
  await expect(row.locator('.name')).toHaveCSS('pointer-events', 'none');
  await expect(tooltip).not.toContainText('archived-session-opaque-c0ffee');
  await expect(tooltip.locator('em')).toHaveCount(0);
  await expectBottomStartAlignment(trigger, tooltip);

  await row.locator('.unarchive').hover();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'Unarchive Session' }),
  ).toBeVisible();
  await expect(tooltip).toBeHidden();

  await page.getByRole('button', { name: 'Show Sessions' }).click();
  await expect(
    page.locator('.session-row', { hasText: longTitle }),
  ).toBeVisible();
});
