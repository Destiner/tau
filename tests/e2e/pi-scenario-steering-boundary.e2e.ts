import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

test.use({ pausedClock: true });

const scenarioUrl = '/?test-scenario=saved-session-steering-boundary';
const initialPrompt = 'Hold the tool turn';
const steeringMessages = [
  'Steering message one',
  'Steering message two',
  'Steering message three',
];
const followUpMessages = ['Follow-up message one', 'Follow-up message two'];

async function waitForGate(page: Page, gate: string): Promise<void> {
  await page.evaluate(async (name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(name);
  }, gate);
}

async function releaseGate(page: Page, gate: string): Promise<void> {
  await page.evaluate((name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    scenario.releaseGate(name);
  }, gate);
}

function userRows(page: Page, text: string): ReturnType<Page['locator']> {
  return page.locator('article.message.user', { hasText: text });
}

async function waitForQueueUpdates(page: Page, count: number): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__TAU_PI_SCENARIO__
            ?.timeline()
            .filter(
              (entry) =>
                entry.kind === 'output' &&
                entry.output === 'event main@current queue_update',
            ).length ?? 0,
      ),
    )
    .toBeGreaterThanOrEqual(count);
}

test('delivers held steering together and follow-ups sequentially without duplicate transcript rows', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill(initialPrompt);
  await composer.press('Enter');
  await waitForGate(page, 'tool-turn-held');
  await releaseGate(page, 'tool-turn-held');

  for (const [index, message] of steeringMessages.entries()) {
    await composer.fill(message);
    await composer.press('Enter');
    await waitForQueueUpdates(page, index + 1);
  }
  await waitForGate(page, 'steering-queued');
  await expect(page.locator('.message-queue .queue-chip')).toHaveCount(3);

  await releaseGate(page, 'steering-queued');
  await waitForGate(page, 'steering-boundary-delivered');
  for (const message of steeringMessages) {
    await expect(userRows(page, message)).toHaveCount(1);
  }
  await expect(page.locator('.message-queue')).toHaveCount(0);

  await releaseGate(page, 'steering-boundary-delivered');
  for (const [index, message] of followUpMessages.entries()) {
    await composer.fill(message);
    await composer.press('Meta+Enter');
    await waitForQueueUpdates(page, steeringMessages.length + index + 2);
  }
  await waitForGate(page, 'follow-ups-queued');
  await expect(page.locator('.message-queue .queue-chip')).toHaveCount(2);

  await releaseGate(page, 'follow-ups-queued');
  await waitForGate(page, 'first-follow-up-hydrated');
  await expect(userRows(page, followUpMessages[0]!)).toHaveCount(1);
  await expect(userRows(page, followUpMessages[1]!)).toHaveCount(0);
  await expect(page.locator('.message-queue .queue-chip')).toHaveCount(1);

  await releaseGate(page, 'first-follow-up-hydrated');
  await waitForGate(page, 'second-follow-up-hydrated');
  for (const message of [...steeringMessages, ...followUpMessages]) {
    await expect(userRows(page, message)).toHaveCount(1);
  }
  await expect(page.locator('.message-queue')).toHaveCount(0);

  await releaseGate(page, 'second-follow-up-hydrated');
  await expect(
    page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()),
  ).resolves.toMatchObject({ ok: true });
});
