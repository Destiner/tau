import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

test.use({ pausedClock: true });

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

async function expectOutwardPulse(page: Page): Promise<void> {
  const divider = page.locator(
    '.message.compaction:not(.transient-compaction) .compaction-divider',
  );
  await expect(divider).toHaveClass(/pulse/);
  const animations = await divider.evaluate((element) => {
    const rules = Array.from(element.querySelectorAll('.rule'));
    return rules.map((rule) => {
      const pseudo = getComputedStyle(rule, '::after');
      const name = pseudo.animationName;
      const keyframes = Array.from(document.styleSheets)
        .flatMap((sheet) => Array.from(sheet.cssRules))
        .find(
          (entry): entry is CSSKeyframesRule =>
            entry instanceof CSSKeyframesRule && entry.name === name,
        );
      const frames = keyframes ? Array.from(keyframes.cssRules) : [];
      return {
        name: name.replace(/-[0-9a-f]{8}$/, ''),
        start: (frames[0] as CSSKeyframeRule | undefined)?.style.transform,
        end: (frames[frames.length - 1] as CSSKeyframeRule | undefined)?.style
          .transform,
      };
    });
  });
  expect(animations).toEqual([
    { name: 'pulse-left', start: 'translate(110%)', end: 'translate(-440%)' },
    { name: 'pulse-right', start: 'translate(-110%)', end: 'translate(440%)' },
  ]);
}

async function expectActiveCompaction(page: Page): Promise<void> {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const stop = page.getByRole('button', { name: 'Stop Pi' });

  const compaction = page.locator('.transient-compaction');
  await expect(compaction).toHaveText('compacting');
  await expect(compaction.locator('.compaction-divider')).not.toHaveClass(
    /pulse/,
  );
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
  await expectOutwardPulse(page);
  await page.clock.runFor(650);
  await expect(permanent.locator('.compaction-divider')).not.toHaveClass(
    /pulse/,
  );
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
  await page.clock.runFor(1);
  await page.getByRole('button', { name: 'Send Message' }).click();

  await waitForGate(page, 'missed-start-reconciled');
  await expectActiveCompaction(page);

  await releaseGate(page, 'missed-start-reconciled');
  await expect(page.locator('.transient-compaction')).toHaveCount(0);
  await expect(page.locator('.message.compaction')).toHaveCount(1);
  await expect(page.locator('.message.compaction')).toHaveText('compacted');
  await expectOutwardPulse(page);
  await expect(composer).toHaveValue('');
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeVisible();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});

test('keeps the completed divider still with reduced motion', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(scenarioUrl);
  await page.getByRole('textbox', { name: 'Message Pi' }).fill(firstPrompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'compaction-started');
  await releaseGate(page, 'compaction-started');
  await waitForGate(page, 'compaction-ended-continuing');

  const divider = page.locator('.message.compaction .compaction-divider');
  await expect(divider).toHaveClass(/pulse/);
  expect(
    await divider
      .locator('.rule')
      .evaluateAll((rules) =>
        rules.map((rule) => getComputedStyle(rule, '::after').animationName),
      ),
  ).toEqual(['none', 'none']);

  await releaseGate(page, 'compaction-ended-continuing');
  await page.clock.runFor(1);
  await page.getByRole('textbox', { name: 'Message Pi' }).fill(secondPrompt);
  await page.getByRole('button', { name: 'Send Message' }).click();
  await waitForGate(page, 'missed-start-reconciled');
  await releaseGate(page, 'missed-start-reconciled');
  expect(
    (await page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()))?.ok,
  ).toBe(true);
});
