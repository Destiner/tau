import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-auto-retry';

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

test('shows reviewed retry copy beside Stop without shifting the composer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await waitForGate(page, 'before-auto-retry');

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const stop = page.getByRole('button', { name: 'Stop Pi' });
  const composerBefore = await composer.boundingBox();
  const stopBefore = await stop.boundingBox();

  await releaseGate(page, 'before-auto-retry');
  await waitForGate(page, 'auto-retry-visible');

  const retry = page.getByRole('status', { name: /^Retrying\./ });
  await expect(retry).toHaveText('Retrying…');
  await expect(page.getByText('Retrying…', { exact: true })).toHaveCount(1);
  await expect(page.locator('.status-row')).toHaveCount(0);
  await expect(stop).toBeEnabled();
  await expect(composer).toHaveValue('');
  expect(await composer.boundingBox()).toEqual(composerBefore);
  expect(await stop.boundingBox()).toEqual(stopBefore);

  await retry.focus();
  await expect(
    page.locator('.ui-tooltip').filter({
      hasText: 'The model provider is receiving too many requests.',
    }),
  ).toBeVisible();
  await expect(page.locator('body')).not.toContainText(
    'RAW_RETRY_PROVIDER_PAYLOAD',
  );

  await releaseGate(page, 'auto-retry-visible');
  await waitForGate(page, 'auto-retry-finished');
  await expect(retry).toHaveCount(0);
  await expect(stop).toBeFocused();
  expect(await composer.boundingBox()).toEqual(composerBefore);
  expect(await stop.boundingBox()).toEqual(stopBefore);

  await releaseGate(page, 'auto-retry-finished');
});
