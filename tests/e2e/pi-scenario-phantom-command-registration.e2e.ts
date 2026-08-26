import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=phantom-command-registration';
const command = '/mcp';
const sessionName = 'MCP workflow';
const syncGate = 'before-streaming-command-sync';

test('registers a command-created session only when real agent work begins', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();
  await expect(composer).toBeEnabled();

  await composer.fill(command);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await page.evaluate(async (gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(gate);
  }, syncGate);

  await expect(page.getByLabel('Transcript')).toHaveCount(0);

  await page.evaluate(async (gate) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.releaseGate(gate);
  }, syncGate);

  await expect(page.getByRole('heading', { name: sessionName })).toBeVisible();
  const sessionRow = page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .locator('button[aria-current="page"]');
  await expect(sessionRow).toHaveCount(1);
  await expect(sessionRow).toContainText(sessionName);
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeEnabled();
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(1);
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Transcript')).toContainText(
    'Real agent work started.',
  );

  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(diagnostics.timeline?.slice(-1)).toEqual([
    expect.objectContaining({
      kind: 'output',
      output: 'event phantom@current message_update',
    }),
  ]);
});
