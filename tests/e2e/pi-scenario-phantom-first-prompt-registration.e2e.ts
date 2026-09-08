import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=phantom-first-prompt-registration';
const prompt = 'Keep the first prompt visible';
const sessionName = 'First prompt';

interface VisibilityTracker {
  promptSeen: boolean;
  sessionSeen: boolean;
  promptMissingAfterSeen: boolean;
  sessionMissingAfterSeen: boolean;
}

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

async function expectContinuousVisibility(page: Page): Promise<void> {
  const tracker = await page.evaluate(
    () =>
      (
        window as Window & {
          __TAU_FIRST_PROMPT_VISIBILITY__?: VisibilityTracker;
        }
      ).__TAU_FIRST_PROMPT_VISIBILITY__,
  );
  expect(tracker).toEqual({
    promptSeen: true,
    sessionSeen: true,
    promptMissingAfterSeen: false,
    sessionMissingAfterSeen: false,
  });
}

test('keeps a phantom first prompt and held sidebar row visible through registration', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();
  await composer.fill(prompt);

  const selectedSession = page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .locator('.session-row.selected');
  await expect(selectedSession).toHaveCount(1);
  await page.evaluate((message) => {
    const tracker: VisibilityTracker = {
      promptSeen: false,
      sessionSeen: false,
      promptMissingAfterSeen: false,
      sessionMissingAfterSeen: false,
    };
    const record = (): void => {
      const transcript =
        document.querySelector('[aria-label="Transcript"]')?.textContent ?? '';
      const promptVisible = transcript.includes(message);
      const sessionVisible =
        document.querySelectorAll('.projects-body .session-row.selected')
          .length === 1;
      if (tracker.promptSeen && !promptVisible) {
        tracker.promptMissingAfterSeen = true;
      }
      if (tracker.sessionSeen && !sessionVisible) {
        tracker.sessionMissingAfterSeen = true;
      }
      tracker.promptSeen ||= promptVisible;
      tracker.sessionSeen ||= sessionVisible;
    };
    new MutationObserver(record).observe(document.body, {
      attributes: true,
      childList: true,
      characterData: true,
      subtree: true,
    });
    (
      window as Window & {
        __TAU_FIRST_PROMPT_VISIBILITY__?: VisibilityTracker;
      }
    ).__TAU_FIRST_PROMPT_VISIBILITY__ = tracker;
    record();
  }, prompt);

  await selectedSession.hover();
  await page.evaluate(() => {
    const sessionList = document.querySelector<HTMLElement>('.projects-body');
    if (!sessionList) throw new Error('Expected the project session list.');
    // The pointer may already be inside after creating the session, so capture
    // the hold explicitly rather than depending on another pointerenter.
    sessionList.dispatchEvent(
      new PointerEvent('pointerenter', { pointerType: 'mouse' }),
    );
  });
  // Focus returns without moving the pointer off the held row.
  await composer.press('Enter');

  const userMessage = page
    .getByLabel('Transcript')
    .locator('article.message.user');
  await waitForGate(page, 'before-first-prompt-identity');
  await expect(userMessage).toHaveCount(1);
  await expect(userMessage).toContainText(prompt);
  await expect(selectedSession).toHaveCount(1);
  await expectContinuousVisibility(page);

  await releaseGate(page, 'before-first-prompt-identity');
  await waitForGate(page, 'after-empty-first-prompt-hydration');
  await expect(userMessage).toHaveCount(1);
  await expect(userMessage).toContainText(prompt);
  await expect(selectedSession).toHaveCount(1);
  await expectContinuousVisibility(page);

  await releaseGate(page, 'after-empty-first-prompt-hydration');
  await waitForGate(page, 'after-first-prompt-registration');
  await expect(page.getByRole('heading', { name: sessionName })).toBeVisible();
  await expect(userMessage).toHaveCount(1);
  await expect(userMessage).toContainText(prompt);
  await expect(selectedSession).toHaveCount(1);
  await expectContinuousVisibility(page);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__TAU_PI_SCENARIO__?.hasRegisteredSession(
            'session-first-prompt',
          ) ?? false,
      ),
    )
    .toBe(true);
  expect(
    await page.evaluate(() =>
      window.__TAU_PI_SCENARIO__?.nativeInvocationCount('set_active_session'),
    ),
  ).toBe(2);

  await releaseGate(page, 'after-first-prompt-registration');
  await expect(userMessage).toHaveCount(1);
  await expect(userMessage).toContainText(prompt);
  await expect(selectedSession).toHaveCount(1);
  await expectContinuousVisibility(page);

  const diagnostics = await page.evaluate(() => ({
    verification: window.__TAU_PI_SCENARIO__?.verify(),
    timeline: window.__TAU_PI_SCENARIO__?.timeline(),
  }));
  expect(diagnostics.verification?.ok).toBe(true);
  expect(diagnostics.timeline?.at(-1)).toMatchObject({
    kind: 'output',
    output: 'runtime-event phantom@current exited',
  });
});
