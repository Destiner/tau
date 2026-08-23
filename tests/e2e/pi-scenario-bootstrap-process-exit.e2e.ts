import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap-process-exit';
const beforeFailure = 'before-bootstrap-process-failure';
const afterError = 'after-bootstrap-bridge-error';
const afterExit = 'after-bootstrap-process-exit';
const connectionFailure =
  'The Pi connection failed. Select the session again to reconnect.';
const processFailure =
  'The Pi process stopped unexpectedly. Select the session again to reconnect.';
const rawSentinel = /RAW_(?:STDERR|BRIDGE_ERROR|EXIT)_SECRET_SENTINEL/;

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

async function expectNoRawFailure(page: Page): Promise<void> {
  await expect(page.locator('body')).not.toContainText(rawSentinel);
}

test('bounds a bootstrap process exit and reconnects from the session row', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await waitForGate(page, beforeFailure);

  await expectNoRawFailure(page);
  await releaseGate(page, beforeFailure);
  await waitForGate(page, afterError);

  await expect(page.getByRole('status')).toHaveText(connectionFailure);
  await expectNoRawFailure(page);
  await releaseGate(page, afterError);
  await waitForGate(page, afterExit);

  await expect(page.getByRole('status')).toHaveText(processFailure);
  await expect(page.getByText('Loading', { exact: true })).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeDisabled();
  await expectNoRawFailure(page);

  await releaseGate(page, afterExit);
  await page.getByRole('button', { name: /^Main\b/ }).click();

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await expect(page.getByRole('status')).toHaveCount(0);
  await expect(page.getByText('Loading', { exact: true })).toHaveCount(0);
  await composer.fill('Recovery is available');
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeEnabled();
  await composer.fill('');
  await expectNoRawFailure(page);

  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(JSON.stringify(diagnostics.timeline)).not.toContain('RAW_');
  expect(
    diagnostics.timeline?.filter(
      (entry) =>
        entry.kind === 'runtime-bound' && entry.runtime === 'recovered-main',
    ),
  ).toHaveLength(1);
});
