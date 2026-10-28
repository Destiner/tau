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

async function switchSession(page: Page, name: string): Promise<void> {
  await pressSessionSwitcher(page);
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name: new RegExp(`^${name}(?:\\s|$)`) })
    .click();
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
}

async function openPalette(page: Page): Promise<void> {
  await modifiedShortcut(page, 'k');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
}

for (const interaction of ['pointer', 'keyboard'] as const) {
  test(`pending prompt allows project collapse by ${interaction}`, async ({
    page,
  }) => {
    await page.goto(scenarioUrl);
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    await expect(prompt).toBeVisible();
    await prompt.getByRole('option', { name: 'patch' }).focus();
    const collapse = page.getByRole('button', {
      name: 'Collapse Tau fixture',
      exact: true,
    });
    if (interaction === 'pointer') await collapse.click();
    else {
      await collapse.focus();
      await collapse.press('ArrowLeft');
    }
    const expand = page.getByRole('button', {
      name: 'Expand Tau fixture',
      exact: true,
    });
    await expect(expand).toBeVisible();
    await expect(expand).toBeFocused();
    await expect(prompt).toBeVisible();
    await expect(
      page.getByRole('heading', { name: 'Main', exact: true }),
    ).toBeVisible();
    await expand.press(interaction === 'keyboard' ? 'ArrowRight' : 'Enter');
    await expect(collapse).toBeFocused();
    expect(
      (await page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()))?.ok,
    ).toBe(true);
  });
}

for (const source of ['palette'] as const) {
  test(`pending prompt allows active session rename from ${source}`, async ({
    page,
  }) => {
    await page.goto(scenarioUrl);
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    await expect(prompt).toBeVisible();
    await prompt.getByRole('option', { name: 'patch' }).focus();
    await openPalette(page);
    await page
      .getByRole('combobox', { name: 'Command Palette' })
      .fill('Rename Session');
    await page.getByRole('option', { name: /Rename Session/ }).click();
    const name = page.getByRole('textbox', {
      name: 'Session Name',
      exact: true,
    });
    await expect(name).toBeFocused();
    await name.fill('Uncommitted pending prompt rename');
    await expect(prompt).toBeVisible();
    await name.press('Escape');
    await expect(name).toHaveCount(0);
    await expect(
      page.getByRole('heading', { name: 'Main', exact: true }),
    ).toBeVisible();
    await expect(prompt).toBeVisible();
    expect(
      (await page.evaluate(() => window.__TAU_PI_SCENARIO__?.verify()))?.ok,
    ).toBe(true);
  });
}

for (const target of ['active', 'inactive'] as const) {
  test(`pending prompt allows ${target} project rename from context without switching sessions`, async ({
    page,
  }) => {
    await page.goto(
      '/?test-scenario=saved-session-extension-prompt-project-switching',
    );
    const prompt = page.getByRole('dialog', {
      name: 'Which label should the release carry?',
    });
    await expect(prompt).toBeVisible();
    await switchSession(page, 'Other Main');
    await switchSession(page, 'Main');
    await expect(prompt).toBeVisible();
    const project = page.getByRole('button', {
      name: target === 'active' ? 'Tau fixture' : 'Other fixture',
      exact: true,
    });
    await project.click({ button: 'right' });
    await page
      .getByRole('menuitem', { name: 'Rename Project…', exact: true })
      .click();
    const dialog = page.getByRole('dialog', {
      name: 'Rename Project',
      exact: true,
    });
    const name = dialog.getByRole('textbox', {
      name: 'Project Name',
      exact: true,
    });
    await expect(name).toBeFocused();
    await expect(name).toHaveValue(
      target === 'active' ? 'Tau fixture' : 'Other fixture',
    );
    await name.fill('Uncommitted project title');
    await expect(
      page.getByRole('dialog', {
        name: 'Which label should the release carry?',
        includeHidden: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole('heading', {
        name: 'Main',
        exact: true,
        includeHidden: true,
      }),
    ).toBeVisible();
    await name.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(project).toBeFocused();
    await expect(prompt).toBeVisible();
  });
}

test('pending prompt allows project rename from palette without cancelling its answer', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-input');
  const input = page.getByRole('textbox', {
    name: 'Release note label',
    exact: true,
  });
  await expect(input).toBeVisible();
  await input.fill('Keep this pending answer');
  await openPalette(page);
  await page
    .getByRole('combobox', { name: 'Command Palette' })
    .fill('Rename Project');
  await page.getByRole('option', { name: /Rename Project/ }).click();
  const dialog = page.getByRole('dialog', {
    name: 'Rename Project',
    exact: true,
  });
  const name = dialog.getByRole('textbox', {
    name: 'Project Name',
    exact: true,
  });
  await expect(name).toBeFocused();
  await name.fill('Uncommitted project title');
  await name.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(input).toBeFocused();
  await expect(input).toHaveValue('Keep this pending answer');
});

test('inactive session context does not rename the active pending session', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-prompt-cycling');
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  await expect(prompt).toBeVisible();
  await page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .getByRole('button', { name: /^(?:Unread )?Backup(?: |$)/ })
    .click({ button: 'right' });
  await expect(page.getByRole('menu')).toBeVisible();
  await expect(
    page.getByRole('menuitem', { name: /Rename Session/ }),
  ).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('textbox', { name: 'Session Name', exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole('heading', { name: 'Main', exact: true }),
  ).toBeVisible();
  await expect(prompt).toBeVisible();
  await switchSession(page, 'Backup');
  await switchSession(page, 'Main');
  await expect(prompt).toBeVisible();
});

for (const { method, title } of [
  { method: 'input', title: 'Release note label' },
  { method: 'editor', title: 'Release notes' },
]) {
  test(`collapse and rename preserve a pending ${method} draft`, async ({
    page,
  }) => {
    await page.goto(`/?test-scenario=saved-session-extension-${method}`);
    const input = page.getByRole('textbox', { name: title, exact: true });
    await expect(input).toBeVisible();
    await input.fill('Preserve this unfinished release answer');
    const collapse = page.getByRole('button', {
      name: 'Collapse Tau fixture',
      exact: true,
    });
    await collapse.click();
    const expand = page.getByRole('button', {
      name: 'Expand Tau fixture',
      exact: true,
    });
    await expect(expand).toBeFocused();
    await expect(input).toHaveValue('Preserve this unfinished release answer');
    await expand.press('Enter');
    await openPalette(page);
    await page
      .getByRole('combobox', { name: 'Command Palette' })
      .fill('Rename Session');
    await page.getByRole('option', { name: /Rename Session/ }).click();
    const name = page.getByRole('textbox', {
      name: 'Session Name',
      exact: true,
    });
    await expect(name).toBeFocused();
    await name.fill('Uncommitted rename');
    await name.press('Escape');
    await expect(input).toHaveValue('Preserve this unfinished release answer');
    await input.focus();
    await expect(input).toBeFocused();
    await expect(
      page.getByRole('heading', { name: 'Main', exact: true }),
    ).toBeVisible();
  });
}

test('prompt expiry does not steal focus from an unfinished session rename', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  const prompt = page.getByRole('dialog', {
    name: 'Which label should the release carry?',
  });
  await expect(prompt).toBeVisible();
  await modifiedShortcut(page, 'Shift+r');
  const name = page.getByRole('textbox', { name: 'Session Name', exact: true });
  await expect(name).toBeFocused();
  await name.fill('Unfinished rename survives expiry');
  await page.clock.fastForward(promptTimeout);
  await expect(prompt).toHaveCount(0);
  await expect(name).toBeFocused();
  await expect(name).toHaveValue('Unfinished rename survives expiry');
  await name.press('Escape');
  await expect(
    page.getByRole('heading', { name: 'Main', exact: true }),
  ).toBeVisible();
});

test('retains a pending question after a native New Session event', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
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
  await page.goto(scenarioUrl);

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

test('switches sessions across projects from a pending extension prompt and restores its focus', async ({
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

  await pressSessionSwitcher(page);
  const picker = page.getByRole('dialog', { name: 'Switch Session' });
  await expect(picker.getByRole('group')).toHaveCount(2);
  await picker.getByRole('option', { name: 'Other Main' }).click();
  await expect(page.getByRole('heading', { name: 'Other Main' })).toBeVisible();

  await pressSessionSwitcher(page);
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name: /^Main(?: |$)/ })
    .click();
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
  await expect(
    page.getByRole('option', { name: /Choose Model/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Rename Session/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Rename Project/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Collapse Project/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Find in Transcript/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Archive Session/ }),
  ).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(prompt).toBeVisible();
  await expect(option).toBeFocused();
});

test('cycles sessions through palette-only commands while an extension prompt is pending', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-prompt-cycling');
  await expect(page.getByRole('option', { name: 'patch' })).toBeFocused();

  await openPalette(page);
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await search.fill('Next Session');
  await search.press('Enter');
  await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeFocused();

  await openPalette(page);
  await search.fill('Previous Session');
  await search.press('Enter');
  await expect(
    page.getByRole('dialog', { name: 'Which label should the release carry?' }),
  ).toBeVisible();
  await expect(page.getByRole('option', { name: 'patch' })).toBeFocused();
});

test('preserves an editor draft across session navigation', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-extension-editor-navigation');
  const input = page.getByRole('textbox', { name: 'Release notes' });
  await expect(input).toBeFocused();
  await input.fill('Ship the editor draft intact.');

  await emitNativeNewSession(page);
  await expect(
    page.getByRole('heading', { name: 'New Session' }),
  ).toBeVisible();
  await pressSessionSwitcher(page);
  await page.getByRole('option', { name: 'Main' }).click();

  await expect(input).toHaveValue('Ship the editor draft intact.');
  await expect(input).toBeFocused();
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
    await page.goto(scenarioUrl);
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
    await prompt.getByRole('option', { name: 'patch' }).focus();
    await expect(prompt.getByRole('option', { name: 'patch' })).toBeFocused();
    await page.clock.fastForward(promptTimeout - 1);
    await expect(prompt).toBeVisible();
    await expect(composer).toHaveCount(0);
    await page.clock.runFor(1);
    await expect(prompt).toHaveCount(0);
    await expect(composer).toBeFocused();
  });
});
