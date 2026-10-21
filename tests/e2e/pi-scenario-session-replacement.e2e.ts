import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

test.use({ pausedClock: true });

const scenarioUrl = '/?test-scenario=saved-session-command-replacement';
const command = '/mock 42';
const replacementName = '42 • plan';
const replacementGate = 'before-command-replacement-identity';
const hydrationGate = 'before-live-user-hydration';
const assistantGate = 'before-replacement-assistant';
const settledHydrationGate = 'after-settled-hydration';
const injectedPrompt = 'Run phase 42';

test('registers and selects a session replaced by an extension command', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  const sidebar = page.getByRole('complementary', {
    name: 'Projects and Sessions',
  });
  await sidebar.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(composer).toBeEnabled();
  await sidebar.getByRole('button', { name: /^Main\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await composer.fill(command);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, replacementGate);

  await expect(composer).toHaveValue('');
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toHaveCount(0);

  await releaseGate(page, replacementGate);

  await expect(
    page.getByRole('heading', { name: replacementName }),
  ).toBeVisible();
  const replacementRow = sidebar.getByRole('button', {
    name: new RegExp(`^Working ${replacementName}\\b`),
  });
  await expect(replacementRow).toHaveCount(1);
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await waitForGate(page, hydrationGate);

  const userRow = page.locator('article.message.user', {
    hasText: injectedPrompt,
  });
  await expect(userRow).toBeVisible();
  await expect(userRow).toHaveCount(1);
  const projectedId = await userRow.getAttribute('data-message-id');

  await releaseGate(page, hydrationGate);
  await waitForGate(page, assistantGate);
  await expect(userRow).toHaveCount(1);
  await expect(userRow).toHaveAttribute('data-message-id', projectedId ?? '');

  const backupRow = sidebar.getByRole('button', { name: /^Backup\b/ });
  await backupRow.click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await replacementRow.click();
  await expect(
    page.getByRole('heading', { name: replacementName }),
  ).toBeVisible();
  await expect(
    page.getByText('This selection could not be saved. Select it again.'),
  ).toHaveCount(0);
  await expect(userRow).toHaveAttribute('data-message-id', projectedId ?? '');

  await releaseGate(page, assistantGate);
  await expect(page.getByLabel('Transcript')).toContainText(
    'Replacement session started.',
  );
  await waitForGate(page, settledHydrationGate);
  await expect(userRow).toHaveCount(1);
  await expect(userRow).toHaveAttribute('data-message-id', projectedId ?? '');
  await releaseGate(page, settledHydrationGate);

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification).toMatchObject({ ok: true });
});
