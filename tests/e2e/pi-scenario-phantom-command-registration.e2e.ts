import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const scenarioUrl = '/?test-scenario=phantom-command-registration';
const command = '/mcp';
const sessionName = 'MCP workflow';
const syncGate = 'before-streaming-command-sync';
const settlementGate = 'before-assistant-settlement';
const registeredWhileWorkingGate = 'after-message-end-registration';

test('registers a command-created session after assistant message_end while work continues', async ({
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
  await waitForGate(page, syncGate);

  await expect(page.getByLabel('Transcript')).toHaveCount(0);

  await releaseGate(page, syncGate);

  await waitForGate(page, settlementGate);
  await expect(page.getByRole('heading', { name: sessionName })).toBeVisible();
  await expect(page.getByLabel('Transcript')).toContainText(
    'Real agent work started.',
  );
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toHaveCount(0);

  await releaseGate(page, settlementGate);
  await waitForGate(page, registeredWhileWorkingGate);

  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__TAU_PI_SCENARIO__?.hasRegisteredSession('session-mcp') ??
          false,
      ),
    )
    .toBe(true);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeVisible();
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toHaveCount(0);

  await releaseGate(page, registeredWhileWorkingGate);

  const sessionRow = page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .locator('button[aria-current="page"]');
  await expect(sessionRow).toHaveCount(1);
  await expect(sessionRow).toContainText(sessionName);
  await sessionRow.hover();
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toBeVisible();
  await sessionRow.focus();
  await expect(
    page.getByRole('button', { name: `Archive ${sessionName}` }),
  ).toBeVisible();
  await expect(page.getByText(command, { exact: true })).toHaveCount(0);
});
