import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

test.use({ pausedClock: true });

const scenarioUrl = '/?test-scenario=delayed-successor-verification';
const planName = 'docs · RHI-6267 · Plan';
const implementName = 'docs · RHI-6267 · Implement';
const backupDraft = 'Keep this Backup draft while Pi starts Implement';
const planDraft = 'Keep this Plan draft while Pi starts Implement';

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

test('keeps an empty successor alive after delayed verification and preserves navigation drafts', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const sidebar = page.getByRole('complementary', {
    name: 'Projects and Sessions',
  });
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await sidebar.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await composer.fill(backupDraft);
  await sidebar.getByRole('button', { name: /^Main\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await composer.fill('/mock-workflow');
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'plan-registered');

  const planRow = sidebar.locator('button.session-select').filter({
    has: page.locator('.session-title', { hasText: planName }),
  });
  const backupRow = sidebar.getByRole('button', { name: /^Draft Backup\b/ });
  const implementRow = sidebar.locator('button.session-select').filter({
    has: page.locator('.session-title', { hasText: implementName }),
  });
  await releaseGate(page, 'plan-registered');
  await expect(page.getByRole('heading', { name: planName })).toBeVisible();
  await expect(planRow).toHaveCount(1);
  await expect(planRow).toHaveAttribute('aria-current', 'page');
  await composer.fill(planDraft);
  await expect(composer).toHaveValue(planDraft);

  await waitForGate(page, 'before-implement-identity');
  await releaseGate(page, 'before-implement-identity');
  await waitForGate(page, 'before-implement-retry-one');

  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();
  await expect(planRow).toHaveCount(1);
  await expect(implementRow).toHaveCount(1);
  await expect(implementRow).toHaveAttribute('aria-current', 'page');

  await releaseGate(page, 'before-implement-retry-one');
  await page.clock.runFor(1);
  await page.clock.runFor(250);
  await waitForGate(page, 'before-implement-retry-two');

  await releaseGate(page, 'before-implement-retry-two');
  await page.clock.runFor(1);
  await page.clock.runFor(750);
  await waitForGate(page, 'before-implement-retry-three');

  await releaseGate(page, 'before-implement-retry-three');
  await page.clock.runFor(1);
  await page.clock.runFor(1_500);
  await page.clock.runFor(2_000);
  await waitForGate(page, 'before-delayed-successor-start');
  await backupRow.click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(composer).toHaveValue(backupDraft);
  await expect(implementRow).toHaveCount(1);
  await implementRow.click();
  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();
  await composer.fill('Draft for the waiting successor');
  await backupRow.click();
  await implementRow.click();
  await expect(composer).toHaveValue('Draft for the waiting successor');
  await backupRow.click();
  await expect(composer).toHaveValue(backupDraft);
  await expect(implementRow).toHaveCount(1);
  expect(
    await page.evaluate(() =>
      window.__TAU_PI_SCENARIO__?.hasRegisteredSession('session-implement'),
    ),
  ).toBe(false);
  await releaseGate(page, 'before-delayed-successor-start');
  await waitForGate(page, 'implement-running');
  await releaseGate(page, 'implement-running');
  await waitForGate(page, 'implement-materialized');

  const [starts, stops, registrations, hasImplement] = await page.evaluate(
    () => {
      const scenario = window.__TAU_PI_SCENARIO__;
      return [
        scenario?.nativeInvocationCount('start_pi') ?? 0,
        scenario?.nativeInvocationCount('stop_pi') ?? 0,
        scenario?.nativeInvocationCount('register_session') ?? 0,
        scenario?.hasRegisteredSession('session-implement') ?? false,
      ];
    },
  );
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(composer).toHaveValue(backupDraft);
  await expect(planRow).toHaveCount(1);
  expect(starts).toBe(2);
  expect(stops).toBe(0);
  expect(registrations).toBe(5);
  expect(hasImplement).toBe(true);
  await expect(implementRow).toHaveCount(1);
  await releaseGate(page, 'implement-materialized');
  await expect(
    implementRow.getByRole('img', { name: 'Working' }),
  ).toBeVisible();
});
