import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-unacknowledged-abort';
const prompt = 'Stop this fixture';
const partialReply = 'Partial reply.';
const abortConsumedGate = 'abort-request-consumed';
const probeResponseGate = 'before-abort-timeout-probe-response';

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

test('recovers when Pi never acknowledges an abort', async ({ page }) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const send = page.getByRole('button', { name: 'Send message' });
  await expect(composer).toBeEnabled();
  await composer.fill(prompt);
  await send.click();

  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
  await expect(page.getByText(partialReply, { exact: true })).toBeVisible();
  const stop = page.getByRole('button', { name: 'Stop Pi' });
  await expect(stop).toBeEnabled();

  await page.clock.install();
  await stop.click();
  await waitForGate(page, abortConsumedGate);

  await expect(stop).toBeDisabled();
  await expect(
    page.getByRole('status', { name: 'Pi is stopping' }),
  ).toBeVisible();
  const requestsBeforeRepeat = await page.evaluate(
    () =>
      window.__TAU_PI_SCENARIO__
        ?.timeline()
        .filter((entry) => entry.kind === 'request').length ?? 0,
  );
  await stop.evaluate((element) => (element as HTMLButtonElement).click());
  const heldTimeline = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.timeline(),
  );
  expect(
    heldTimeline?.filter(
      (entry) => entry.kind === 'request' && entry.request.type === 'abort',
    ),
  ).toHaveLength(1);
  expect(
    heldTimeline?.filter((entry) => entry.kind === 'request'),
  ).toHaveLength(requestsBeforeRepeat);
  expect(heldTimeline?.at(-1)).toMatchObject({
    kind: 'gate-reached',
    gate: abortConsumedGate,
  });

  await releaseGate(page, abortConsumedGate);
  const positionedGates = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.gates(),
  );
  expect(positionedGates).toEqual([
    {
      name: abortConsumedGate,
      required: true,
      reached: true,
      released: true,
    },
    {
      name: probeResponseGate,
      required: true,
      reached: false,
      released: false,
    },
  ]);

  await page.clock.fastForward(2_000);
  await waitForGate(page, probeResponseGate);
  await expect(stop).toBeDisabled();
  await expect(page.getByText(partialReply, { exact: true })).toBeVisible();

  await releaseGate(page, probeResponseGate);

  await expect(stop).toHaveCount(0);
  await expect(send).toBeVisible();
  await expect(composer).toBeEnabled();
  await expect(page.getByRole('status')).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(page.getByLabel('Tau transcript')).toContainText(prompt);
  await expect(page.getByLabel('Tau transcript')).toContainText(partialReply);
  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
  await expect(page.getByText(partialReply, { exact: true })).toBeVisible();

  await composer.fill('Ready again');
  await expect(send).toBeEnabled();
  await composer.fill('');

  const completed = await page.evaluate(() => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    return {
      verification: scenario.verify(),
      gates: scenario.gates(),
      timeline: scenario.timeline(),
    };
  });
  expect(completed.verification.ok).toBe(true);
  expect(completed.gates).toEqual([
    {
      name: abortConsumedGate,
      required: true,
      reached: true,
      released: true,
    },
    {
      name: probeResponseGate,
      required: true,
      reached: true,
      released: true,
    },
  ]);
  expect(
    completed.timeline.filter(
      (entry) => entry.kind === 'request' && entry.request.type === 'abort',
    ),
  ).toHaveLength(1);
  expect(
    completed.timeline.filter(
      (entry) =>
        entry.kind === 'request' &&
        entry.capture === 'abort-timeout-probe' &&
        entry.request.type === 'get_state',
    ),
  ).toHaveLength(1);
  expect(
    completed.timeline.some(
      (entry) =>
        entry.kind === 'output' &&
        (entry.output.includes('response prompt') ||
          entry.output.includes('agent_settled') ||
          entry.output.includes('response abort')),
    ),
  ).toBe(false);
});
