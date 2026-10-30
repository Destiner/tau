import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=saved-session-prompt-process-exit';
const prompt = 'Fail this fixture';
const partial = 'Partial answer before failure.';
const draft = 'Keep this newer draft';
const rawSentinel = /RAW_(?:STDERR|BRIDGE_ERROR|EXIT)_SECRET_SENTINEL/;

test('preserves interrupted output and draft through explicit local retry', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  const composer = page.locator('textarea[aria-label="Message Pi"]');
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(composer).toBeEnabled();
  await composer.fill('Sibling draft');
  await page.getByRole('button', { name: /^Main\b/ }).click();
  await composer.fill(prompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'before-prompt-process-failure');
  const transcript = page.getByLabel('Transcript');
  await expect(transcript).toContainText(prompt);
  await expect(transcript).toContainText(partial);
  await composer.fill(draft);

  await releaseGate(page, 'before-prompt-process-failure');
  await waitForGate(page, 'after-prompt-bridge-error');
  const status = page.locator('.reconnect-status');
  const retry = status.getByRole('button', { name: 'Retry' });
  await expect(status).toBeVisible();
  await expect(retry).toBeDisabled();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(rawSentinel);

  await releaseGate(page, 'after-prompt-bridge-error');
  await waitForGate(page, 'after-prompt-process-exit');
  await expect(retry).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(transcript).toContainText(partial);
  await releaseGate(page, 'after-prompt-process-exit');

  await page.getByRole('button', { name: /^Draft Backup\b/ }).click();
  await expect(composer).toHaveValue('Sibling draft');
  await expect(status).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(partial);
  await page.getByRole('button', { name: /^Draft Main\b/ }).click();
  await expect(retry).toBeEnabled();
  expect(
    await page.evaluate(
      () =>
        window.__TAU_PI_SCENARIO__
          ?.timeline()
          .filter(
            (entry) =>
              entry.kind === 'runtime-bound' &&
              entry.runtime === 'recovered-main',
          ).length,
    ),
  ).toBe(0);

  await retry.click();
  await waitForGate(page, 'before-recovery-history');
  await expect(
    status.getByRole('button', { name: 'Retrying…' }),
  ).toBeDisabled();
  const newSession = page.getByRole('button', {
    name: 'New Session',
    exact: true,
  });
  await newSession.focus();
  await releaseGate(page, 'before-recovery-history');
  await expect(status).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await expect(newSession).toBeFocused();
  await expect(composer).toHaveValue(draft);
  await expect(transcript).toContainText(prompt);
  await expect(transcript).toContainText(partial);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(rawSentinel);
  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(
    diagnostics.timeline?.filter(
      (entry) =>
        entry.kind === 'runtime-bound' && entry.runtime === 'recovered-main',
    ),
  ).toHaveLength(1);
});
