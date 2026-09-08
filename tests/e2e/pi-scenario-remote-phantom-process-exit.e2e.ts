import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=remote-phantom-prompt-process-exit';
const prompt = 'Keep this remote draft';
const beforeFailure = 'before-remote-phantom-prompt-process-failure';
const afterError = 'after-remote-phantom-prompt-bridge-error';
const afterExit = 'after-remote-phantom-prompt-process-exit';
const connectionFailure =
  'The remote Pi connection failed. Check the connection and try again.';
const processFailure =
  'The remote Pi process stopped unexpectedly. Check the connection and try again.';

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

test('restores a remote phantom first prompt and keeps retry actionable', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();

  await composer.fill(prompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, beforeFailure);
  await expect(page.getByLabel('Transcript')).toContainText(prompt);

  await releaseGate(page, beforeFailure);
  await waitForGate(page, afterError);

  const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('alert')).toHaveText(connectionFailure);
  await expect(page.locator('textarea[aria-label="Message Pi"]')).toHaveValue(
    prompt,
  );
  await expect(page.locator('[aria-label="Transcript"]')).toHaveCount(0);

  await releaseGate(page, afterError);
  await waitForGate(page, afterExit);
  await expect(dialog.getByRole('alert')).toHaveText(processFailure);
  await expect(page.locator('textarea[aria-label="Message Pi"]')).toHaveValue(
    prompt,
  );
  await expect(page.locator('[aria-label="Working"]')).toHaveCount(0);

  await releaseGate(page, afterExit);
  await dialog
    .getByRole('textbox', { name: 'SSH Connection String' })
    .press('Enter');

  await expect(dialog).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await expect(composer).toHaveValue(prompt);
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeEnabled();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
