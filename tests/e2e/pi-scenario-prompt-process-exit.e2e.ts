import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-prompt-process-exit';
const prompt = 'Fail this fixture';
const partial = 'Partial answer before failure.';
const beforeFailure = 'before-prompt-process-failure';
const afterError = 'after-prompt-bridge-error';
const afterExit = 'after-prompt-process-exit';
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

test('keeps a partial failed prompt local to its owning session', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(composer).toBeEnabled();
  // Switching sessions leaves the caret in the composer, not on the session
  // button that was clicked.
  await expect(composer).toBeFocused();
  await page.getByRole('button', { name: /^Main\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await composer.fill(prompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, beforeFailure);

  const transcript = page.getByLabel('Transcript');
  await expect(transcript).toContainText(prompt);
  await expect(transcript).toContainText(partial);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeEnabled();
  await expectNoRawFailure(page);

  await releaseGate(page, beforeFailure);
  await waitForGate(page, afterError);
  await expect(page.getByRole('status')).toHaveText(connectionFailure);
  await expect(transcript).toContainText(prompt);
  await expect(transcript).toContainText(partial);
  await expectNoRawFailure(page);

  await releaseGate(page, afterError);
  await waitForGate(page, afterExit);
  await expect(page.getByRole('status')).toHaveText(processFailure);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(
    page.getByRole('status', { name: /^(?:Working|Stopping)$/ }),
  ).toHaveCount(0);
  await expect(transcript).toContainText(prompt);
  await expect(transcript).toContainText(partial);
  await expect(composer).toHaveValue('');
  await expectNoRawFailure(page);
  await releaseGate(page, afterExit);

  await page.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(page.getByRole('status')).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(prompt);
  await expect(page.locator('body')).not.toContainText(partial);
  await expect(page.locator('body')).not.toContainText(processFailure);
  await expect(composer).toBeEnabled();
  await expectNoRawFailure(page);

  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(JSON.stringify(diagnostics.timeline)).not.toContain('RAW_');
});
