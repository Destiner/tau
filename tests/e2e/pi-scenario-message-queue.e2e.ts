import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-message-queue';
const initialPrompt = 'Start a queueable fixture run';
const steeringMessage = 'Steer toward the focused assertion';
const followUpMessage = 'Summarize after the focused assertion';
const stopMessage = 'Keep this queued when stopping';

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

test('preserves a newer draft on queue rejection and restores the unsent draft only when safe', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-queue-rejection');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Start held work');
  await composer.press('Enter');
  await waitForGate(page, 'streaming-ready');
  await releaseGate(page, 'streaming-ready');

  await composer.fill('Restore me later');
  await composer.press('Enter');
  await waitForGate(page, 'submission-requested');
  await composer.fill('New draft stays');
  await releaseGate(page, 'submission-requested');
  await waitForGate(page, 'submission-rejected');

  await expect(composer).toHaveValue('New draft stays');
  await page.getByRole('button', { name: 'Review Unsent (1)' }).click();
  const restore = page.getByRole('button', { name: 'Restore Draft' });
  await expect(restore).toBeDisabled();
  await expect(
    page.getByText('Clear the composer to restore a draft.'),
  ).toBeVisible();
  await composer.fill('');
  await expect(restore).toBeEnabled();
  await restore.click();
  await expect(composer).toHaveValue('Restore me later');
  await expect(composer).toBeFocused();
  await expect(
    page.getByRole('region', { name: 'Pending messages' }),
  ).toHaveCount(0);
  await releaseGate(page, 'submission-rejected');
  await expect(
    page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()),
  ).resolves.toMatchObject({ ok: true });
});

test('queues busy composer messages and never clears them implicitly', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill(initialPrompt);
  await composer.press('Enter');
  await waitForGate(page, 'streaming-ready');
  await releaseGate(page, 'streaming-ready');

  await composer.fill(steeringMessage);
  await composer.press('Enter');
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.__TAU_PI_SCENARIO__
          ?.timeline()
          .some(
            (entry) =>
              entry.kind === 'output' &&
              entry.output === 'event main@current queue_update',
          ),
      ),
    )
    .toBe(true);
  await composer.fill(followUpMessage);
  await composer.press('Meta+Enter');
  await waitForGate(page, 'queue-rendered');

  const queue = page.locator('.message-queue');
  await expect(
    queue.getByRole('button', { name: `Steering: ${steeringMessage}` }),
  ).toBeVisible();
  await expect(
    queue.getByRole('button', { name: `Follow Up 1: ${followUpMessage}` }),
  ).toBeVisible();
  await expect(queue.locator('.queue-ordinal')).toHaveText('1');
  await expect(queue.locator('.queue-feedback')).toHaveCount(0);

  await releaseGate(page, 'queue-rendered');
  await queue.getByRole('button', { name: 'Clear All' }).click();
  await waitForGate(page, 'clear-requested');

  const requestsWhileClearing = await page.evaluate(
    () =>
      window.__TAU_PI_SCENARIO__
        ?.timeline()
        .filter((entry) => entry.kind === 'request')
        .map((entry) => entry.request.type) ?? [],
  );
  expect(requestsWhileClearing).not.toContain('abort');
  expect(
    requestsWhileClearing.filter((type) => type === 'clear_queue'),
  ).toHaveLength(1);

  await releaseGate(page, 'clear-requested');
  await expect(queue).toHaveCount(0);
  await expect(composer).toBeFocused();

  await composer.fill(stopMessage);
  await composer.press('Enter');
  await waitForGate(page, 'stop-ready');
  await expect(queue).toContainText(stopMessage);

  await releaseGate(page, 'stop-ready');
  await page.getByRole('button', { name: 'Stop Pi' }).click();
  await waitForGate(page, 'stop-requested');
  await expect(queue).toContainText(stopMessage);

  const completed = await page.evaluate(() => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    return scenario.timeline();
  });
  expect(
    completed.filter(
      (entry) =>
        entry.kind === 'request' && entry.request.type === 'clear_queue',
    ),
  ).toHaveLength(1);
  expect(
    completed.filter(
      (entry) => entry.kind === 'request' && entry.request.type === 'abort',
    ),
  ).toHaveLength(1);

  await releaseGate(page, 'stop-requested');
  await expect(
    page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()),
  ).resolves.toMatchObject({ ok: true });
});
