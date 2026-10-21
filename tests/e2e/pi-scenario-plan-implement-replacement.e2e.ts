import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=plan-implement-replacement';
const planName = 'docs · RHI-6267 · Plan';
const implementName = 'docs · RHI-6267 · Implement';
const hydrationGate = 'plan-hydration-lagged';
const identityGate = 'before-implement-identity';
const implementGate = 'implement-active';

test('preserves and selects phases when a delayed probe discovers Implement', async ({
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
  await sidebar.getByRole('button', { name: /^Main\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await composer.fill('/mock-workflow');
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, hydrationGate);
  await expect(page.getByRole('heading', { name: planName })).toBeVisible();
  await releaseGate(page, hydrationGate);
  await waitForGate(page, identityGate);
  const planRow = sidebar.getByRole('button', {
    name: new RegExp(`^${planName}\\b`),
  });
  await expect(page.getByRole('heading', { name: planName })).toBeVisible();
  await expect(planRow).toHaveCount(1);
  await expect(planRow).toHaveAttribute('aria-current', 'page');
  await releaseGate(page, identityGate);
  await waitForGate(page, implementGate);

  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();
  const implementRow = sidebar.getByRole('button', {
    name: new RegExp(`${implementName}\\b`),
  });
  await expect(planRow).toHaveCount(1);
  await expect(implementRow).toHaveCount(1);
  await expect(implementRow).toHaveAttribute('aria-current', 'page');
  await expect(planRow).not.toHaveAttribute('aria-current', 'page');
  const phaseOrder = await sidebar
    .locator('.session-title')
    .filter({ hasText: 'docs · RHI-6267' })
    .allTextContents();
  expect(phaseOrder.indexOf(implementName)).toBeLessThan(
    phaseOrder.indexOf(planName),
  );

  await releaseGate(page, implementGate);
  await sidebar.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(planRow).toHaveCount(1);
  await expect(implementRow).toHaveCount(1);
  await planRow.click();
  await expect(page.getByRole('heading', { name: planName })).toBeVisible();
  await expect(page.getByLabel('Transcript')).toContainText(
    'Approved implementation plan.',
  );

  await implementRow.click();
  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();
  await expect(implementRow).toHaveAttribute('aria-current', 'page');
  await expect(planRow).not.toHaveAttribute('aria-current', 'page');
  await expect(planRow).toHaveCount(1);
  await expect(implementRow).toHaveCount(1);

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification).toMatchObject({ ok: true });
});
