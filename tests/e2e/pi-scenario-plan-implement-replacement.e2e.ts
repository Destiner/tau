import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=plan-implement-replacement';
const planName = 'docs · RHI-6267 · Plan';
const implementName = 'docs · RHI-6267 · Implement';
const implementGate = 'implement-active';

test('preserves Plan when settlement verification discovers Implement', async ({
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
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    implementGate,
  );

  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();
  const planRow = sidebar.getByRole('button', {
    name: new RegExp(`^${planName}\\b`),
  });
  const implementRow = sidebar.getByRole('button', {
    name: new RegExp(`${implementName}\\b`),
  });
  await expect(planRow).toHaveCount(1);
  await expect(implementRow).toHaveCount(1);

  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    implementGate,
  );
  await sidebar.getByRole('button', { name: /^Backup\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await planRow.click();
  await expect(page.getByRole('heading', { name: planName })).toBeVisible();
  await expect(page.getByLabel('Transcript')).toContainText(
    'Approved implementation plan.',
  );

  await implementRow.click();
  await expect(
    page.getByRole('heading', { name: implementName }),
  ).toBeVisible();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification).toMatchObject({ ok: true });
});
