import { promptTimeout } from '../support/pi-scenario/saved-session-extension-prompt';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-extension-prompt';

/**
 * A question an extension asks belongs to the turn that asked it, so it is
 * rendered in the transcript and the composer stands down until it is answered:
 * one scrolling view rather than a transcript above a prompt with its own.
 */
test('renders an extension prompt in the transcript in place of the composer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const transcript = page.getByLabel('Tau transcript');
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });

  await expect(prompt).toBeVisible();
  // Nothing to send while the session is waiting on an answer.
  await expect(composer).toHaveCount(0);
  // The prompt is inside the transcript's scroll region, not below it.
  await expect(transcript.getByRole('dialog')).toBeVisible();
  await expect(prompt.getByRole('option', { name: 'minor' })).toBeVisible();
  // The turn that asked is still above the question.
  await expect(transcript).toContainText('Working through the checklist now.');

  // The first option holds focus, so the answer is a keystroke away.
  await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();

  // Pi stops waiting, and the composer comes back with the caret in it.
  await expect(prompt).toHaveCount(0, { timeout: promptTimeout * 2 });
  await expect(composer).toBeFocused();
});

test('renders a prompt asked of a session that holds nothing yet', async ({
  page,
}) => {
  await page.goto('/?test-scenario=empty-session-extension-prompt');

  const transcript = page.getByLabel('Tau transcript');
  const prompt = transcript.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });

  // An empty session gives its pane to the composer, which the question takes
  // over: the transcript is there, holding the prompt and nothing else.
  await expect(prompt).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toHaveCount(
    0,
  );
  await expect(transcript.locator('.message')).toHaveCount(1);
});

test('Escape dismisses only a dismissible layer above the prompt', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  await expect(prompt).toBeVisible();

  // The workflow question cannot be dismissed without cancelling it, so an
  // unclaimed Escape leaves both it and its current answer alone.
  await page.keyboard.press('Escape');
  await expect(prompt).toBeVisible();

  const trigger = page.getByRole('button', { name: 'Open project' });
  await trigger.click();
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();
  const remoteDialog = page.getByRole('dialog', { name: 'SSH connection' });
  await expect(remoteDialog).toBeVisible();

  await page.keyboard.press('Escape');

  await expect(remoteDialog).toHaveCount(0);
  await expect(prompt).toBeVisible();
  await expect(trigger).toBeFocused();
});
