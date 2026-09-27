import { promptTimeout } from '../support/pi-scenario/saved-session-extension-prompt';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-extension-prompt';

test.use({ pausedClock: true });

/**
 * A question an extension asks belongs to the turn that asked it, so it is
 * rendered in the transcript and the composer stands down until it is answered:
 * one scrolling view rather than a transcript above a prompt with its own.
 */
test('renders a prompt asked of a session that holds nothing yet', async ({
  page,
}) => {
  await page.goto('/?test-scenario=empty-session-extension-prompt');

  const transcript = page.getByLabel('Transcript');
  const prompt = transcript.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });

  await expect(prompt).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toHaveCount(
    0,
  );
  await expect(transcript.locator('.message')).toHaveCount(1);
});

test('Escape cancels a focused prompt through its originating runtime', async ({
  page,
}) => {
  await page.goto(
    '/?test-scenario=saved-session-extension-prompt-escape-cancellation',
  );

  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();
  await page.keyboard.press('Escape');

  await expect(prompt).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();
  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});

test('keeps an extension question under a dismissible layer until it times out', async ({
  page,
}) => {
  await test.step('Escape dismisses only a dismissible layer above the prompt', async () => {
    await page.goto(scenarioUrl);
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    await expect(prompt).toBeVisible();
    const trigger = page.getByRole('button', { name: 'Open Project' });
    await trigger.click();
    await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();
    const remoteDialog = page.getByRole('dialog', { name: 'SSH Connection' });
    await expect(remoteDialog).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(remoteDialog).toHaveCount(0);
    await expect(prompt).toBeVisible();
    await expect(trigger).toBeFocused();
  });

  await test.step('renders an extension prompt in the transcript in place of the composer', async () => {
    await page.getByRole('option', { name: 'patch' }).focus();
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    const transcript = page.getByLabel('Transcript');
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    await expect(prompt).toBeVisible();
    await expect(composer).toHaveCount(0);
    await expect(transcript.getByRole('dialog')).toBeVisible();
    await expect(prompt.getByRole('option', { name: 'minor' })).toBeVisible();
    await expect(transcript).toContainText(
      'Working through the checklist now.',
    );
    await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();
    await page.clock.fastForward(promptTimeout - 1);
    await expect(prompt).toBeVisible();
    await expect(composer).toHaveCount(0);
    await page.clock.runFor(1);
    await expect(prompt).toHaveCount(0);
    await expect(composer).toBeFocused();
  });
});
