import {
  followUp,
  longSentence,
  multiline,
  unbroken,
} from '../support/pi-scenario/saved-session-queue-pills';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-queue-pills';

async function gate(
  page: import('@playwright/test').Page,
  name: string,
): Promise<void> {
  await page.evaluate(async (gateName) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(gateName);
  }, name);
}

async function release(
  page: import('@playwright/test').Page,
  name: string,
): Promise<void> {
  await page.evaluate(async (gateName) => {
    await window.__TAU_PI_SCENARIO__?.releaseGate(gateName);
  }, name);
}

test('sizes pills intrinsically with a bounded cap and compact, readable double-digit badges', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await gate(page, 'pills-rendered');
  const queue = page.getByRole('region', { name: 'Pending messages' });
  const geometry = await queue.evaluate((element) => {
    const chips = [...element.querySelectorAll<HTMLElement>('.queue-chip')];
    const badge = element.querySelector<HTMLElement>('.queue-ordinal');
    const lastBadge = element.querySelector<HTMLElement>(
      '.queue-chip:last-child .queue-ordinal',
    );
    if (!badge || !lastBadge) throw new Error('Expected badges');
    return {
      widths: chips.map((chip) => chip.getBoundingClientRect().width),
      heights: chips.map((chip) => chip.getBoundingClientRect().height),
      badgeHeight: badge.getBoundingClientRect().height,
      badgePadding: getComputedStyle(badge).paddingLeft,
      lastBadgeWidth: lastBadge.getBoundingClientRect().width,
      lastBadgeScrollWidth: lastBadge.scrollWidth,
      snippets: chips.map((chip) => {
        const snippet = chip.querySelector<HTMLElement>('.queue-snippet')!;
        return {
          height: snippet.getBoundingClientRect().height,
          scrollWidth: snippet.scrollWidth,
          clientWidth: snippet.clientWidth,
        };
      }),
    };
  });
  expect(geometry.widths[0]!).toBeLessThan(geometry.widths[1]!);
  expect(geometry.widths[3]!).toBeLessThan(geometry.widths[4]!);
  expect(Math.max(...geometry.widths)).toBeLessThanOrEqual(180.5);
  expect(geometry.badgeHeight).toBe(14);
  expect(geometry.badgePadding).toBe('2px');
  expect(geometry.lastBadgeWidth).toBeGreaterThan(14);
  expect(geometry.lastBadgeScrollWidth).toBeLessThanOrEqual(
    geometry.lastBadgeWidth + 1,
  );
  expect(geometry.snippets[1]!.scrollWidth).toBeGreaterThan(
    geometry.snippets[1]!.clientWidth,
  );
  expect(geometry.snippets[2]!.scrollWidth).toBeGreaterThan(
    geometry.snippets[2]!.clientWidth,
  );
  expect(geometry.snippets[5]!.scrollWidth).toBeGreaterThan(
    geometry.snippets[5]!.clientWidth,
  );
  expect(new Set(geometry.heights).size).toBe(1);
  await expect(
    queue
      .getByRole('button', { name: `Follow Up 11: ${followUp[10]}` })
      .locator('.queue-ordinal'),
  ).toHaveText('11');
  await queue.getByRole('button', { name: `Steering: ${unbroken}` }).focus();
  await expect(page.getByRole('tooltip')).toContainText(unbroken);
  await queue
    .getByRole('button', { name: `Follow Up 3: ${multiline}` })
    .focus();
  await expect(page.getByRole('tooltip')).toContainText(
    'Second line with more context',
  );
  await release(page, 'pills-rendered');
  await gate(page, 'pills-renumbered');
  await release(page, 'pills-renumbered');
  await expect(
    page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()),
  ).resolves.toMatchObject({ ok: true });
});

test('keeps narrow rails scrollable, previews bounded, and focus and order correct after updates', async ({
  page,
}) => {
  await page.clock.install();
  await page.setViewportSize({ width: 420, height: 540 });
  await page.goto(scenarioUrl);
  await gate(page, 'pills-rendered');
  const queue = page.getByRole('region', { name: 'Pending messages' });
  const rail = queue.locator('.queue-rail');
  await expect(
    queue.getByRole('button', { name: `Follow Up 11: ${followUp[10]}` }),
  ).toBeAttached();
  const bounds = await rail.evaluate((element) => ({
    right: element.getBoundingClientRect().right,
    scrollWidth: element.scrollWidth,
    clientWidth: element.clientWidth,
    documentWidth: document.documentElement.scrollWidth,
  }));
  expect(bounds.right).toBeLessThanOrEqual(420);
  expect(bounds.documentWidth).toBeLessThanOrEqual(420);
  expect(bounds.scrollWidth).toBeGreaterThan(bounds.clientWidth);
  await rail.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  const last = queue.getByRole('button', {
    name: `Follow Up 11: ${followUp[10]}`,
  });
  await expect(last).toBeInViewport();
  await last.focus();
  const preview = page.getByRole('tooltip').filter({ hasText: followUp[10] });
  await expect(preview).toBeVisible();
  await expect(preview.locator('strong')).toHaveText('Follow Up');
  expect(
    await preview.evaluate((element) => element.getBoundingClientRect().right),
  ).toBeLessThanOrEqual(420);
  await last.press('Escape');
  await expect(preview).toHaveCount(0);
  await expect(last).toBeFocused();

  const first = queue.getByRole('button', {
    name: `Follow Up 1: ${followUp[0]}`,
  });
  await first.focus();
  await expect(
    page.getByRole('tooltip').filter({ hasText: followUp[0] }),
  ).toBeVisible();
  await first.press('Escape');
  await first.hover();
  await page.clock.runFor(200);
  const hoverPreview = page
    .getByRole('tooltip')
    .filter({ hasText: followUp[0] });
  await expect(hoverPreview).toBeVisible();
  await hoverPreview.hover();
  await page.clock.runFor(100);
  await expect(hoverPreview).toBeVisible();
  await release(page, 'pills-rendered');
  await gate(page, 'pills-renumbered');
  await expect(
    page.getByRole('tooltip').filter({ hasText: followUp[0] }),
  ).toHaveCount(0);
  await expect(
    queue.getByRole('button', { name: `Follow Up 1: ${longSentence}` }),
  ).toBeAttached();
  await expect(
    queue
      .getByRole('button', { name: `Follow Up 10: ${followUp[10]}` })
      .locator('.queue-ordinal'),
  ).toHaveText('10');
  await release(page, 'pills-renumbered');
  await expect(
    page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()),
  ).resolves.toMatchObject({ ok: true });
});
