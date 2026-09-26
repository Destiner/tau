import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-tooltip';

async function waitForFixture(page: Page): Promise<void> {
  await page.waitForFunction(() =>
    Boolean(window.__TAU_SESSION_TOOLTIP_FIXTURE__),
  );
}
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
  const tooltip = page.locator('.ui-tooltip', { hasText: 'Markdown preview' });
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Working',
  );
  const markdown = tooltip.locator('.session-tooltip-name');
  await expect(markdown.locator('strong')).toHaveText('Markdown preview');
  await expect(markdown.locator('p code')).toHaveText('inline code');
  await expect(markdown.locator('li')).toHaveCount(2);
  await expect(markdown.locator('li li')).toHaveText('Nested item');
  await expect(markdown.locator('pre')).toContainText('const ready = true;');
  await expect(tooltip.locator('.session-tooltip-date')).toHaveText(
    '2 hours ago',
  );
  await expect(tooltip).not.toContainText('working-session-opaque-7fb4d9');
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

  await waitForFixture(page);
  const replacementTitle = '**Updated**\n\nSecond paragraph';
  await page.evaluate((title) => {
    const fixture = window.__TAU_SESSION_TOOLTIP_FIXTURE__;
    if (!fixture) throw new Error('Expected session-tooltip fixture API.');
    fixture.setStatus('draft');
    fixture.setTitle(title);
  }, replacementTitle);

  const row = page.locator('.session-row', { hasText: replacementTitle });
  await row.locator('.session-select').hover();
  const tooltip = page.locator('.ui-tooltip.session-tooltip');
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Unsent draft',
  );
  await expect(tooltip.locator('.session-tooltip-name strong')).toHaveText(
    'Updated',
  );
  await expect(tooltip.locator('.session-tooltip-name p')).toHaveCount(2);
  await expect(tooltip).not.toContainText('working-session-opaque-7fb4d9');

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

test('sanitizes unsafe Markdown and bounds truncated multiline previews', async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 320 });
  await page.goto(fixtureUrl);
  const row = page.locator('.session-row').first();
  await waitForFixture(page);
  const source =
    'Safe [web link](https://example.com) [unsafe](javascript:alert(1)) <img src=x onerror=alert(1)>\n\n' +
    Array.from({ length: 40 }, (_, index) => `line ${index}`).join('  \n');
  await page.evaluate((text) => {
    window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setMarkdown(text);
  }, source);
  await row.locator('.session-select').hover();
  const tooltip = page.locator('.ui-tooltip.session-tooltip');
  await expect(tooltip).toBeVisible();
  await expect(
    tooltip.locator('[onerror], a[href^="javascript:"]'),
  ).toHaveCount(0);
  await expect(tooltip.locator('a[href="https://example.com"]')).toHaveCount(1);
  const bounds = await tooltip.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.height).toBeLessThanOrEqual(320);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(360);
  await page.evaluate(() => {
    window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setMarkdown(
      '```js\n' + 'x'.repeat(235),
    );
  });
  await expect(tooltip.locator('.session-tooltip-name')).toContainText('x');
  await expect(tooltip).not.toContainText('working-session-opaque-7fb4d9');
});

test('keeps the end of tall active and archived previews reachable', async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 520 });
  await page.goto(fixtureUrl);
  await waitForFixture(page);
  const source = 'line\n'.repeat(46) + 'final line';
  expect(source).toHaveLength(240);
  await page.evaluate((markdown) => {
    const fixture = window.__TAU_SESSION_TOOLTIP_FIXTURE__;
    fixture?.setMarkdown(markdown);
    fixture?.setArchivedMarkdown(markdown);
  }, source);

  async function expectReachableDate(): Promise<void> {
    const tooltip = page.locator('.ui-tooltip.session-tooltip');
    await expect(tooltip).toBeVisible();
    await expect(tooltip.locator('.session-tooltip-name')).toContainText(
      'final line',
    );
    const scroll = await tooltip.evaluate((element) => {
      const overflow = element.scrollHeight - element.clientHeight;
      element.scrollTop = element.scrollHeight;
      return overflow;
    });
    expect(scroll).toBeGreaterThan(0);
    await expect
      .poll(async () => {
        const bounds = await tooltip.boundingBox();
        const date = await tooltip
          .locator('.session-tooltip-date')
          .boundingBox();
        if (!bounds || !date) return false;
        return (
          bounds.y >= 0 &&
          bounds.y + bounds.height <= 520 + 1 &&
          date.y >= bounds.y &&
          date.y + date.height <= bounds.y + bounds.height &&
          date.y + date.height <= 520 + 1
        );
      })
      .toBe(true);
  }

  await page.locator('.session-row').first().locator('.session-select').hover();
  await expectReachableDate();

  await page.getByTestId('outside-sidebar').hover();
  await expect(page.locator('.ui-tooltip.session-tooltip')).toBeHidden();
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  await page.locator('.archived-list .row .copy').first().hover();
  await expectReachableDate();
});

test('opens Markdown web links externally without navigating the app', async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.__TAU_TOOLTIP_OPENED_URLS__ = [];
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: async (command: string, payload?: { url?: string }) => {
        if (command === 'plugin:opener|open_url')
          window.__TAU_TOOLTIP_OPENED_URLS__?.push(payload?.url ?? '');
        return null;
      },
    } as typeof window.__TAURI_INTERNALS__;
  });
  await page.goto(fixtureUrl);
  await waitForFixture(page);
  await page.evaluate(() => {
    window.__TAU_SESSION_TOOLTIP_FIXTURE__?.setMarkdown(
      '[Website](https://example.com)',
    );
  });
  await page.locator('.session-row').first().locator('.session-select').hover();
  await page.locator('.session-tooltip-name a').click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_TOOLTIP_OPENED_URLS__))
    .toEqual(['https://example.com']);
  expect(page.url()).toContain(fixtureUrl);
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
  const tooltip = page.locator('.ui-tooltip', { hasText: 'Archived notes' });
  await expect(tooltip).toBeVisible();
  await expect(tooltip.locator('.session-tooltip-status')).toHaveText(
    'Archived',
  );
  await expect(tooltip.locator('.session-tooltip-name p')).toHaveCount(2);
  await expect(tooltip.locator('.session-tooltip-name em')).toHaveText('notes');
  await expect(tooltip.locator('.session-tooltip-name code')).toHaveText(
    'code',
  );
  await expect(tooltip.locator('.session-tooltip-date')).toHaveText(
    '3 weeks ago',
  );
  await expect(row.locator('.name')).toHaveCSS('pointer-events', 'none');
  await expect(tooltip).not.toContainText('archived-session-opaque-c0ffee');
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
