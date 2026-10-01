import type { Page } from '@playwright/test';

import { promptTimeout } from '../support/pi-scenario/saved-session-extension-prompt';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-extension-prompt';

test.use({ pausedClock: true });

async function emitNativeNewSession(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'tau://new-session',
      payload: undefined,
    });
  });
}

async function modifiedShortcut(page: Page, key: string): Promise<void> {
  await page.keyboard.press(
    (await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform)))
      ? `Meta+${key}`
      : `Control+${key}`,
  );
}

async function pressSessionSwitcher(page: Page): Promise<void> {
  await modifiedShortcut(page, 'p');
}

async function openPalette(page: Page): Promise<void> {
  await modifiedShortcut(page, 'k');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
}

test('retains a pending question after a native New Session event', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-prompt-navigation');
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();

  await emitNativeNewSession(page);
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();

  await pressSessionSwitcher(page);
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name: 'Main' })
    .click();
  await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();
});

test('executes palette New Session and Switch Session while an extension prompt is pending', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-prompt-navigation');

  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  const option = prompt.getByRole('option', { name: 'patch' });
  await expect(option).toBeFocused();

  await openPalette(page);
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await search.fill('New Session');
  await search.press('Enter');
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();

  await openPalette(page);
  await search.fill('Switch Session');
  await search.press('Enter');
  const picker = page.getByRole('dialog', { name: 'Switch Session' });
  await expect(picker).toBeVisible();
  await picker.getByRole('option', { name: 'Main' }).click();

  await expect(prompt).toBeVisible();
  await expect(option).toBeFocused();
});

test('switches projects from a pending extension prompt and restores its focus', async ({
  page,
}) => {
  await page.goto(
    '/?test-scenario=saved-session-extension-prompt-project-switching',
  );

  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  const option = prompt.getByRole('option', { name: 'patch' });
  await expect(option).toBeFocused();

  await modifiedShortcut(page, 'Shift+p');
  const projects = page.getByRole('dialog', { name: 'Switch Project' });
  await expect(projects).toBeVisible();
  await projects.getByRole('option', { name: /Other fixture/ }).click();
  await expect(page.getByRole('heading', { name: 'Other Main' })).toBeVisible();

  await modifiedShortcut(page, 'Shift+p');
  await page.getByRole('option', { name: /Tau fixture/ }).click();
  await expect(prompt).toBeVisible();
  await expect(option).toBeFocused();
});

for (const navigation of ['session', 'project', 'boundary'] as const) {
  test(`restores prompt focus after palette ${navigation} navigation stays put`, async ({
    page,
  }) => {
    await page.goto(scenarioUrl);
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    const option = prompt.getByRole('option', { name: 'patch' });
    await expect(option).toBeFocused();

    if (navigation === 'session') {
      await pressSessionSwitcher(page);
      const picker = page.getByRole('dialog', { name: 'Switch Session' });
      await picker.getByRole('option', { name: 'Main' }).press('Enter');
    } else if (navigation === 'project') {
      await modifiedShortcut(page, 'Shift+p');
      const picker = page.getByRole('dialog', { name: 'Switch Project' });
      await picker.getByRole('option', { name: /Tau fixture/ }).press('Enter');
    } else {
      await openPalette(page);
      const search = page.getByRole('combobox', { name: 'Command Palette' });
      await search.fill('Next Session');
      await search.press('Enter');
    }

    await expect(
      page.getByRole('dialog', { name: /Switch|Command Palette/ }),
    ).toHaveCount(0);
    await expect(prompt).toBeVisible();
    await expect(option).toBeFocused();
    const verification = await page.evaluate(() =>
      window.__TAU_PI_SCENARIO__?.verify(),
    );
    expect(verification?.ok).toBe(true);
  });
}

test('keeps a typed answer focused when reselecting its session', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-input-navigation');
  const input = page.getByRole('textbox', { name: 'Release note label' });
  await expect(input).toBeFocused();
  await input.fill('rc.2');

  await emitNativeNewSession(page);
  await pressSessionSwitcher(page);
  await page.getByRole('option', { name: 'Main' }).press('Enter');
  await expect(input).toBeFocused();

  await pressSessionSwitcher(page);
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name: 'Main' })
    .press('Enter');

  await expect(input).toBeFocused();
  await expect(input).toHaveValue('rc.2');
  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification?.ok).toBe(true);
});

test('opens Switch Session with Cmd+P while an extension prompt is pending', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(
    page.getByRole('dialog', { name: 'Which label should the release carry?' }),
  ).toBeVisible();

  await pressSessionSwitcher(page);

  const picker = page.getByRole('dialog', { name: 'Switch Session' });
  await expect(picker).toBeVisible();
  await expect(picker.getByRole('option')).toHaveCount(1);
});

test('keeps navigation commands available in the palette and restores prompt focus on Escape', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  const option = prompt.getByRole('option', { name: 'patch' });
  await expect(option).toBeFocused();

  await openPalette(page);
  await expect(page.getByRole('option', { name: /New Session/ })).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Switch Session/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Switch Project/ }),
  ).toBeVisible();
  await expect(page.getByRole('option', { name: /Choose Model/ })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole('option', { name: /Archive Session/ }),
  ).toHaveCount(0);

  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(prompt).toBeVisible();
  await expect(option).toBeFocused();
});

test('opens Switch Project with Cmd+Shift+P while an extension prompt is pending', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  const option = prompt.getByRole('option', { name: 'patch' });
  await expect(option).toBeFocused();

  await modifiedShortcut(page, 'Shift+p');

  const picker = page.getByRole('dialog', { name: 'Switch Project' });
  await expect(picker).toBeVisible();
  await expect(
    picker.getByRole('option', { name: /Tau fixture/ }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(option).toBeFocused();
});

test('cycles sessions with Ctrl+Tab while an extension prompt is pending', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-prompt-cycling');
  await expect(page.getByRole('option', { name: 'patch' })).toBeFocused();

  await page.keyboard.press('Control+Tab');
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();

  await page.keyboard.press('Control+Shift+Tab');
  await expect(
    page.getByRole('dialog', { name: 'Which label should the release carry?' }),
  ).toBeVisible();
  await expect(page.getByRole('option', { name: 'patch' })).toBeFocused();
});

for (const textPrompt of [
  {
    scenario: 'saved-session-extension-input-navigation',
    title: 'Release note label',
    draft: 'rc.2',
  },
  {
    scenario: 'saved-session-extension-editor-navigation',
    title: 'Release notes',
    draft: 'Ship the editor draft intact.',
  },
]) {
  test(`preserves a pending extension ${textPrompt.scenario.includes('editor') ? 'editor' : 'input'} draft across session navigation`, async ({
    page,
  }) => {
    await page.goto(`/?test-scenario=${textPrompt.scenario}`);
    const prompt = page.getByRole('dialog', { name: textPrompt.title });
    const input = prompt.getByRole('textbox', { name: textPrompt.title });
    await expect(input).toBeFocused();
    await input.fill(textPrompt.draft);

    await emitNativeNewSession(page);
    await expect(
      page.getByRole('heading', { name: 'New Session' }),
    ).toBeVisible();
    await pressSessionSwitcher(page);
    await page.getByRole('option', { name: 'Main' }).click();

    await expect(prompt).toBeVisible();
    await expect(input).toHaveValue(textPrompt.draft);
    await expect(input).toBeFocused();
  });
}

test('does not resurrect an input prompt that expires while its session is hidden', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-input-navigation');
  await expect(
    page.getByRole('textbox', { name: 'Release note label' }),
  ).toBeFocused();
  await emitNativeNewSession(page);
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();

  await page.clock.fastForward(promptTimeout);
  await pressSessionSwitcher(page);
  await page.getByRole('option', { name: 'Main' }).click();

  await expect(
    page.getByRole('dialog', { name: 'Release note label' }),
  ).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();
});

test('keeps navigation blocked by a real dialog above an extension prompt', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();
  await expect(
    page.getByRole('dialog', { name: 'SSH Connection' }),
  ).toBeVisible();

  await emitNativeNewSession(page);

  await expect(page.getByRole('heading', { name: 'New Session' })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole('dialog', { name: 'SSH Connection' }),
  ).toBeVisible();
});

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
