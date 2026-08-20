import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=phantom-command-registration';
const command = '/mcp';
const sessionName = 'MCP workflow';
const syncGate = 'before-streaming-command-sync';

test('registers a command-created session while its identity sync is streaming', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: 'New session', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'New session' }),
  ).toBeVisible();
  await expect(composer).toBeEnabled();

  await composer.fill(command);
  await page.getByRole('button', { name: 'Send message' }).click();
  await page.evaluate(async (gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(gate);
  }, syncGate);

  await expect(page.getByLabel('Tau transcript')).toHaveCount(0);

  await page.evaluate(async (gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.releaseGate(gate);
  }, syncGate);

  await expect(page.getByRole('heading', { name: sessionName })).toBeVisible();
  const sessionRow = page
    .getByRole('complementary', { name: 'Projects and sessions' })
    .locator('button[aria-current="page"]');
  await expect(sessionRow).toHaveCount(1);
  await expect(sessionRow).toContainText(sessionName);
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeEnabled();
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(1);
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Tau transcript')).toHaveCount(0);

  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(diagnostics.timeline?.slice(-2)).toEqual([
    expect.objectContaining({ kind: 'gate-released', gate: syncGate }),
    expect.objectContaining({
      kind: 'output',
      output: 'response get_state -> $streaming-command-sync',
    }),
  ]);
});
