import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap-process-exit';
const rawSentinel = /RAW_(?:STDERR|BRIDGE_ERROR|EXIT)_SECRET_SENTINEL/;

test('offers explicit retry after a saved-session bootstrap process exit', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await waitForGate(page, 'before-bootstrap-process-failure');
  await releaseGate(page, 'before-bootstrap-process-failure');
  await waitForGate(page, 'after-bootstrap-bridge-error');

  const status = page.locator('.reconnect-status');
  const retry = status.getByRole('button', { name: 'Retry' });
  await expect(status).toBeVisible();
  await expect(retry).toBeDisabled();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(rawSentinel);

  await releaseGate(page, 'after-bootstrap-bridge-error');
  await waitForGate(page, 'after-bootstrap-process-exit');
  await expect(retry).toBeEnabled();
  await expect(page.getByText('Loading', { exact: true })).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await releaseGate(page, 'after-bootstrap-process-exit');
  await expect(status).toBeVisible();
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
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await releaseGate(page, 'before-recovery-history');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(status).toHaveCount(0);
  await expect(composer).toBeEnabled();
  await expect(composer).toBeFocused();
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
