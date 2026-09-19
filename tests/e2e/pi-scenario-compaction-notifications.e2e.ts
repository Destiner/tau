import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-compaction-notifications';

async function waitForGate(page: Page, gate: string): Promise<void> {
  await page.waitForFunction(() => Boolean(window.__TAU_PI_SCENARIO__));
  await page.evaluate(async (name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(name);
  }, gate);
}

async function releaseGate(page: Page, gate: string): Promise<void> {
  await page.evaluate(async (name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.releaseGate(name);
  }, gate);
}

async function visibleRows(page: Page): Promise<string[]> {
  return page.locator('.message-window > .message').evaluateAll((rows) =>
    rows.map((row) => {
      const kind = ['compaction', 'assistant', 'notice', 'error'].find(
        (value) => row.classList.contains(value),
      );
      return `${kind ?? 'unknown'}:${row.textContent?.trim() ?? ''}`;
    }),
  );
}

test('clears compacted notifications and preserves fresh ordering', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await waitForGate(page, 'before-successful-compaction');

  for (const text of [
    'Old informational notice',
    'Old warning notice',
    'Old error notice',
  ]) {
    await expect(
      page.locator('.message.notice').filter({ hasText: text }),
    ).toHaveCount(1);
  }
  await expect(
    page.locator('.message.error').filter({
      hasText: 'The conversation could not be shortened.',
    }),
  ).toHaveCount(1);

  await releaseGate(page, 'before-successful-compaction');
  await waitForGate(page, 'compacted-hydration-pending');

  await expect(page.locator('.message.notice')).toHaveCount(1);
  await expect(page.locator('.message.error')).toHaveCount(0);
  await expect
    .poll(() => visibleRows(page))
    .toEqual([
      'assistant:Output before fresh notice.',
      'notice:WarningFresh warning notice',
      'assistant:Output after fresh notice.',
    ]);

  await releaseGate(page, 'compacted-hydration-pending');
  await waitForGate(page, 'compacted-notifications-reconciled');

  await expect(page.locator('.message.compaction')).toHaveText('compacted');
  await expect
    .poll(() => visibleRows(page))
    .toEqual([
      'compaction:compacted',
      'assistant:Output before fresh notice.',
      'notice:WarningFresh warning notice',
      'assistant:Output after fresh notice.',
    ]);
  for (const text of [
    'Old informational notice',
    'Old warning notice',
    'Old error notice',
  ]) {
    await expect(page.getByText(text, { exact: true })).toHaveCount(0);
  }

  await releaseGate(page, 'compacted-notifications-reconciled');
  await expect
    .poll(async () => page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()))
    .toMatchObject({ ok: true });
});
