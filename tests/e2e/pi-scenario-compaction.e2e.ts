import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-compaction';
const firstPrompt = 'Compact this fixture';
const secondPrompt = 'Miss the compaction start';

async function waitForGate(page: Page, gate: string): Promise<void> {
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

async function expectActiveCompaction(page: Page): Promise<void> {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const stop = page.getByRole('button', { name: 'Stop Pi' });

  await expect(page.locator('.transient-compaction')).toHaveText('compacting');
  await expect(stop).toBeVisible();
  await expect(stop).toBeDisabled();
  await expect(composer).toBeEnabled();

  await stop.evaluate((element) => (element as HTMLButtonElement).click());
  const aborts = await page.evaluate(
    () =>
      window.__TAU_PI_SCENARIO__
        ?.timeline()
        .filter(
          (entry) => entry.kind === 'request' && entry.request.type === 'abort',
        ).length ?? 0,
  );
  expect(aborts).toBe(0);
}

test('shows non-interruptible compaction from events and reconciled state', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill(firstPrompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'compaction-started');

  await expectActiveCompaction(page);
  await composer.fill(secondPrompt);
  await expect(composer).toHaveValue(secondPrompt);

  await releaseGate(page, 'compaction-started');
  await expect(page.locator('.transient-compaction')).toHaveCount(0);
  await expect(page.locator('.message.compaction')).toHaveCount(1);
  await expect(page.locator('.message.compaction')).toHaveText('compacted');
  await expect(composer).toHaveValue(secondPrompt);
  await page.getByRole('button', { name: 'Send Message' }).click();

  await waitForGate(page, 'missed-start-reconciled');
  await expectActiveCompaction(page);

  await releaseGate(page, 'missed-start-reconciled');
  await expect(page.locator('.transient-compaction')).toHaveCount(0);
  await expect(page.locator('.message.compaction')).toHaveCount(1);
  await expect(page.locator('.message.compaction')).toHaveText('compacted');
  await expect(composer).toHaveValue('');
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeVisible();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
