import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-compaction';
const firstPrompt = 'Compact this fixture';
const secondPrompt = 'Miss the compaction start';

async function waitForGate(page: Page, gate: string): Promise<void> {
  await page.evaluate(async (name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.waitForGate(name);
  }, gate);
}

async function releaseGate(page: Page, gate: string): Promise<void> {
  await page.evaluate(async (name) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Expected the browser Pi scenario API.');
    await scenario.releaseGate(name);
  }, gate);
}

async function expectActiveCompaction(page: Page): Promise<void> {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const stop = page.getByRole('button', { name: 'Stop Pi' });

  const compaction = page.locator('.transient-compaction');
  await expect(compaction).toHaveText('compacting');
  await expect
    .poll(async () => {
      const content = await page
        .locator('.message-window .message:last-child .markdown')
        .boundingBox();
      const divider = await compaction
        .locator('.compaction-divider')
        .boundingBox();
      if (!content || !divider) return -1;
      return Math.round(divider.y - (content.y + content.height));
    })
    .toBe(38);
  await expect(stop).toBeVisible();
  await expect(stop).toBeDisabled();
  await expect(composer).toBeEnabled();

  await stop.evaluate((element) => (element as HTMLButtonElement).click());
  const aborts = await page.evaluate(
    () =>
      window.__TAU_PI_SCENARIO__
        ?.timeline()
        .filter(
          (entry) => entry.kind === 'request' && entry.request.type === 'abort',
        ).length ?? 0,
  );
  expect(aborts).toBe(0);
}

test('shows non-interruptible compaction from events and reconciled state', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill(firstPrompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'compaction-started');

  await expectActiveCompaction(page);
  await composer.fill(secondPrompt);
  await expect(composer).toHaveValue(secondPrompt);

  await releaseGate(page, 'compaction-started');
  await waitForGate(page, 'compaction-ended-continuing');
  await expect(page.locator('.transient-compaction')).toHaveCount(0);
  const permanent = page.locator('.message.compaction');
  const retained = page
    .locator('.message.user')
    .filter({ hasText: firstPrompt });
  const continued = page
    .locator('.message.assistant')
    .filter({ hasText: 'Still working.' })
    .last();
  await expect(permanent).toHaveCount(1);
  await expect(permanent).toHaveText('compacted');
  await expect(retained).toHaveCount(1);
  await expect(continued).toBeVisible();
  await expect
    .poll(() =>
      page
        .locator('.message-window > .message')
        .evaluateAll((rows) =>
          rows.map((row) =>
            ['compaction', 'user', 'assistant'].find((kind) =>
              row.classList.contains(kind),
            ),
          ),
        ),
    )
    .toEqual(['compaction', 'user', 'assistant']);
  await expect(composer).toHaveValue(secondPrompt);

  await releaseGate(page, 'compaction-ended-continuing');
  await page.getByRole('button', { name: 'Send Message' }).click();

  await waitForGate(page, 'missed-start-reconciled');
  await expectActiveCompaction(page);

  await releaseGate(page, 'missed-start-reconciled');
  await expect(page.locator('.transient-compaction')).toHaveCount(0);
  await expect(page.locator('.message.compaction')).toHaveCount(1);
  await expect(page.locator('.message.compaction')).toHaveText('compacted');
  await expect(composer).toHaveValue('');
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeVisible();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});
