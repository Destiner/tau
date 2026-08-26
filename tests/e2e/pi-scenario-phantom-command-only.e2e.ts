import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=phantom-command-only';
const sessionName = 'Usage only';

test('removes a command-only session without registering it', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await composer.fill('/usage');
  await page.getByRole('button', { name: 'Send Message' }).click();

  await expect(page.getByRole('heading', { name: sessionName })).toBeVisible();
  await expect(page.getByText('Fixture usage is 10%.')).toBeVisible();
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toContainText(
    'Fixture usage is 10%.',
  );

  await page.getByRole('button', { name: /^Main / }).click();

  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Usage only / })).toHaveCount(
    0,
  );

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
