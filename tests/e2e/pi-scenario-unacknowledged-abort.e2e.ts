import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=saved-session-unacknowledged-abort';
const prompt = 'Stop this fixture';
const partialReply = 'Partial reply.';
const abortConsumedGate = 'abort-request-consumed';
const probeResponseGate = 'before-abort-timeout-probe-response';

test('recovers when Pi never acknowledges an abort', async ({ page }) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const send = page.getByRole('button', { name: 'Send Message' });
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
  await expect(page.getByRole('status', { name: 'Stopping' })).toBeVisible();
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
  await expect(page.getByLabel('Transcript')).toContainText(prompt);
  await expect(page.getByLabel('Transcript')).toContainText(partialReply);
  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
  await expect(page.getByText(partialReply, { exact: true })).toBeVisible();

  await composer.fill('Ready again');
  await expect(send).toBeEnabled();
  await composer.fill('');

  await expect
    .poll(() => page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify().ok))
    .toBe(true);
});
