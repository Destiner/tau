import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-command-replacement';
const command = '/mock 42';
const replacementName = '42 • plan';
const replacementGate = 'before-command-replacement-identity';

test('registers and selects a session replaced by an extension command', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await composer.fill(command);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await page.evaluate((gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    return scenario.waitForGate(gate);
  }, replacementGate);

  await expect(composer).toHaveValue('');
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toHaveCount(0);

  const pausedTimeline = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.timeline(),
  );
  expect(pausedTimeline?.slice(-3)).toEqual([
    expect.objectContaining({
      kind: 'output',
      output: 'response prompt -> $command-prompt',
    }),
    expect.objectContaining({
      kind: 'request',
      capture: 'command-identity-probe',
      request: { type: 'get_state' },
    }),
    expect.objectContaining({
      kind: 'gate-reached',
      gate: replacementGate,
    }),
  ]);

  await page.evaluate((gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    return scenario.releaseGate(gate);
  }, replacementGate);

  await expect(
    page.getByRole('heading', { name: replacementName }),
  ).toBeVisible();
  const sidebar = page.getByRole('complementary', {
    name: 'Projects and Sessions',
  });
  const replacementRow = sidebar.getByRole('button', {
    name: new RegExp(`^${replacementName}\\b`),
  });
  const oldRow = sidebar.getByRole('button', { name: /^Main\b/ });
  await expect(replacementRow).toHaveCount(1);
  await expect(replacementRow).toHaveAttribute('aria-current', 'page');
  await expect(oldRow).toHaveCount(1);
  await expect(oldRow).not.toHaveAttribute('aria-current', 'page');
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toContainText(
    'Replacement session started.',
  );
  await expect(composer).toBeEnabled();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification).toMatchObject({ ok: true });
});
