import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=phantom-command-replacement';
const command = '/mock 42';
const replacementName = '42 • plan';
const promptReplyGate = 'before-command-prompt-reply';
const commandSyncGate = 'before-command-sync-state-reply';
const hydrationGate = 'before-live-user-hydration';
const assistantGate = 'before-replacement-assistant';
const settledHydrationGate = 'after-settled-hydration';

test.use({ pausedClock: true });

test('keeps a fresh command successor running after its phantom identity is replaced', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const sidebar = page.getByRole('complementary', {
    name: 'Projects and Sessions',
  });
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();

  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();

  await composer.fill(command);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    promptReplyGate,
  );

  await sidebar.getByRole('button', { name: /^Main\b/ }).click();
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await composer.fill('Draft for Main');
  await expect(
    sidebar.getByRole('button', { name: /^New Session\b/ }),
  ).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    promptReplyGate,
  );
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    commandSyncGate,
  );

  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(
    sidebar.getByRole('button', { name: /^New Session\b/ }),
  ).toHaveCount(1);
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    commandSyncGate,
  );
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    hydrationGate,
  );
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    hydrationGate,
  );
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    assistantGate,
  );

  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    assistantGate,
  );
  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.waitForGate(gate),
    settledHydrationGate,
  );

  await page.evaluate(
    (gate) => window.__TAU_PI_SCENARIO__?.releaseGate(gate),
    settledHydrationGate,
  );

  const replacementRow = sidebar.getByRole('button', {
    name: new RegExp(`^(?:Unread )?${replacementName}\\b`),
  });
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(composer).toHaveValue('Draft for Main');
  await expect(replacementRow).toHaveCount(1);
  await replacementRow.click();
  await expect(
    page.getByRole('heading', { name: replacementName }),
  ).toBeVisible();
  await expect(page.getByLabel('Transcript')).toContainText(
    'Replacement session started.',
  );

  const [promptRequests, stopCount] = await page.evaluate(() => {
    const scenario = window.__TAU_PI_SCENARIO__;
    return [
      scenario
        ?.timeline()
        .filter(
          (entry) =>
            entry.kind === 'request' && entry.request.type === 'prompt',
        ) ?? [],
      scenario?.nativeInvocationCount('stop_pi') ?? 0,
    ];
  });
  expect(promptRequests).toHaveLength(1);
  expect(stopCount).toBe(0);
});
