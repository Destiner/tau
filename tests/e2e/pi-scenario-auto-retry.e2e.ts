import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=saved-session-auto-retry';

test('shows reviewed retry copy beside the selectors without shifting the composer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await page.waitForFunction(() => Boolean(window.__TAU_PI_SCENARIO__));
  await waitForGate(page, 'before-auto-retry');

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const stop = page.getByRole('button', { name: 'Stop Pi' });
  const effort = page.getByRole('combobox', { name: 'Thinking Effort' });
  const composerBefore = await composer.boundingBox();
  const stopBefore = await stop.boundingBox();
  const effortBefore = await effort.boundingBox();

  await releaseGate(page, 'before-auto-retry');
  await page.waitForFunction(() => Boolean(window.__TAU_PI_SCENARIO__));
  await waitForGate(page, 'auto-retry-visible');

  const retry = page.getByRole('status', { name: /^Retrying\./ });
  await expect(retry).toHaveText('Retrying…');
  await expect(page.getByText('Retrying…', { exact: true })).toHaveCount(1);
  await expect(page.locator('.status-row')).toHaveCount(0);
  await expect(stop).toBeEnabled();
  await expect(composer).toHaveValue('');
  expect(await composer.boundingBox()).toEqual(composerBefore);
  expect(await stop.boundingBox()).toEqual(stopBefore);
  expect(await effort.boundingBox()).toEqual(effortBefore);

  await retry.focus();
  await expect(
    page.locator('.ui-tooltip').filter({
      hasText: 'The model provider is receiving too many requests.',
    }),
  ).toBeVisible();
  await expect(page.locator('body')).not.toContainText(
    'RAW_RETRY_PROVIDER_PAYLOAD',
  );

  await releaseGate(page, 'auto-retry-visible');
  await page.waitForFunction(() => Boolean(window.__TAU_PI_SCENARIO__));
  await waitForGate(page, 'auto-retry-finished');
  await expect(retry).toHaveCount(0);
  await expect(stop).toBeFocused();
  expect(await composer.boundingBox()).toEqual(composerBefore);
  expect(await stop.boundingBox()).toEqual(stopBefore);

  await releaseGate(page, 'auto-retry-finished');
});
