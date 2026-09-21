import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=remote-saved-session-process-exit';
const beforeFailure = 'before-remote-saved-session-process-failure';
const afterError = 'after-remote-saved-session-bridge-error';
const afterExit = 'after-remote-saved-session-process-exit';

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

test('reconnects an established remote session only when requested', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await waitForGate(page, beforeFailure);

  const composer = page.locator('textarea[aria-label="Message Pi"]');
  await composer.fill('Keep this draft');
  await releaseGate(page, beforeFailure);
  await waitForGate(page, afterError);

  const reconnectStatus = page.locator('.reconnect-status');
  const reconnect = reconnectStatus.getByRole('button', { name: 'Reconnect' });
  await expect(reconnectStatus).toContainText(
    'The remote connection was lost. Reconnect to continue.',
  );
  await expect(reconnect).toBeDisabled();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('.status-row')).toHaveCount(0);
  await expect(composer).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(
    'RAW_EXIT_SECRET_SENTINEL',
  );
  await expect(page.locator('[aria-label="Working"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Reconnect' })).toHaveCount(1);

  await releaseGate(page, afterError);
  await waitForGate(page, afterExit);
  await expect(reconnect).toBeEnabled();
  await releaseGate(page, afterExit);
  await reconnect.click();

  await expect(reconnectStatus).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('Keep this draft');

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
