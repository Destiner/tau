import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

test.use({ pausedClock: true });

const scenarioUrl = '/?test-scenario=saved-session-prompt-admission';
const confirmedPrompt = 'Confirm this fixture prompt';
const absentPrompt = 'Reconcile this fixture prompt';
const nextDraft = 'Keep this next draft editable';

test('serializes optimistic ordinary prompt admission', async ({ page }) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const send = page.getByRole('button', { name: 'Send Message' });

  await composer.fill(confirmedPrompt);
  await send.click();
  await waitForGate(page, 'optimistic-pending');

  const confirmedRow = page
    .locator('.message.user')
    .filter({ hasText: confirmedPrompt });
  await expect(confirmedRow).toHaveAttribute('data-pending', 'true');

  await releaseGate(page, 'optimistic-pending');
  await waitForGate(page, 'prompt-confirmed');
  await expect(confirmedRow).not.toHaveAttribute('data-pending', 'true');

  await releaseGate(page, 'prompt-confirmed');
  await page.clock.runFor(1);
  await composer.fill(absentPrompt);
  await expect(send).toBeEnabled();
  await send.click();
  await waitForGate(page, 'before-admission-acknowledgement');
  await releaseGate(page, 'before-admission-acknowledgement');
  await page.clock.runFor(150);
  await waitForGate(page, 'stale-idle-admission');

  const absentRow = page
    .locator('.message.user')
    .filter({ hasText: absentPrompt });
  await expect(absentRow).toHaveAttribute('data-pending', 'true');
  await expect(composer).toBeEnabled();
  await composer.fill(nextDraft);
  await expect(send).toBeDisabled();
  await send.evaluate((element) => (element as HTMLButtonElement).click());

  const promptRequests = await page.evaluate(
    () =>
      window.__TAU_PI_SCENARIO__
        ?.timeline()
        .filter(
          (entry) =>
            entry.kind === 'request' && entry.request.type === 'prompt',
        ).length ?? 0,
  );
  expect(promptRequests).toBe(2);

  await releaseGate(page, 'stale-idle-admission');
  await expect(absentRow).toHaveCount(0);
  await expect(composer).toHaveValue(nextDraft);
  await expect(send).toBeEnabled();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
