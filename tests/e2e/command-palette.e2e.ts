import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap';

async function modShortcut(page: Page, key: string): Promise<string> {
  return (await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform)))
    ? `Meta+${key}`
    : `Control+${key}`;
}

async function paletteShortcut(page: Page): Promise<string> {
  return modShortcut(page, 'k');
}

async function openPalette(page: Page): Promise<void> {
  await expect(
    page.getByRole('button', { name: 'Open Command Palette' }),
  ).toBeVisible();
  await page.keyboard.press(await paletteShortcut(page));
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
}

async function openNestedPage(page: Page, command: string): Promise<void> {
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await search.fill(command);
  await search.press('Enter');
}

test('opens with the platform Mod+K shortcut and filters commands fuzzily in registry order', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  await openPalette(page);
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await expect(search).toBeFocused();

  await search.fill('open');
  await expect(
    page.locator('.command-palette-title').allTextContents(),
  ).resolves.toEqual([
    'Open Project…',
    'Open Local Project',
    'Open Remote Project',
  ]);

  await search.fill('s s');
  await expect(
    page.getByText('Switch Session…', { exact: true }),
  ).toBeVisible();

  await search.fill('');
  await expect(page.getByText('Send Message', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Stop Pi', { exact: true })).toHaveCount(0);
  await expect(
    page.getByText('Clear Queued Messages', { exact: true }),
  ).toHaveCount(0);
});

test('navigates session, project, model, and effort pages and returns from an empty search', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await openPalette(page);

  await openNestedPage(page, 'Switch Session');
  await expect(
    page.getByRole('dialog', { name: 'Switch Session' }),
  ).toBeVisible();
  await expect(page.getByRole('option')).toHaveText('MainCurrent');
  const sessionSearch = page.getByRole('combobox', { name: 'Switch Session' });
  await expect(sessionSearch).toHaveValue('');
  await sessionSearch.press('Backspace');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();

  await openNestedPage(page, 'Switch Project');
  await expect(
    page.getByRole('dialog', { name: 'Switch Project' }),
  ).toBeVisible();
  await expect(page.getByRole('option')).toHaveText(
    'Tau fixture/fixture/tau-project',
  );
  await page.getByRole('button', { name: 'Back' }).click();

  await openNestedPage(page, 'Choose Model');
  await expect(
    page.getByRole('dialog', { name: 'Choose Model' }),
  ).toBeVisible();
  // The deterministic Pi scenario exposes exactly its one fake model.
  await expect(page.getByRole('option')).toHaveText('Alpha');
  await page.getByRole('combobox', { name: 'Choose Model' }).press('Backspace');

  await openNestedPage(page, 'Choose Thinking Effort');
  await expect(
    page.getByRole('dialog', { name: 'Choose Thinking Effort' }),
  ).toBeVisible();
  await expect(page.getByRole('option').allTextContents()).resolves.toEqual([
    'Off',
    'High',
  ]);
});

test('Escape closes the palette and restores focus to its origin', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.focus();
  await openPalette(page);
  await page.keyboard.press('Escape');

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(composer).toBeFocused();
});

test('opening with Mod+K preserves an uncommitted session rename', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  await page.getByRole('button', { name: 'Main', exact: true }).click();
  const name = page.locator('input[aria-label="Session Name"]');
  await name.fill('Uncommitted rename');
  await page.keyboard.press(await paletteShortcut(page));

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await expect(name).toHaveValue('Uncommitted rename');
  await expect(page.getByRole('option', { name: /New Session/ })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole('option', { name: /Switch Session/ }),
  ).toHaveCount(0);

  await page.keyboard.press('Escape');
  await expect(name).toBeVisible();
  await expect(name).toHaveValue('Uncommitted rename');
  await expect(
    page.getByRole('heading', { name: 'Uncommitted rename' }),
  ).toHaveCount(0);
});

test('keeps a dirty remote input when the palette dismisses first', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();
  const remote = page.getByRole('dialog', { name: 'SSH Connection' });
  const connection = page.getByRole('textbox', {
    name: 'SSH Connection String',
  });
  await connection.fill('ssh draft@example');

  await page.keyboard.press(await paletteShortcut(page));
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(remote).toBeVisible();
  await expect(connection).toHaveValue('ssh draft@example');
});

test('preserves a dirty issue report and an open selector under the palette', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page.getByLabel('Projects and Sessions').click();
  await page.keyboard.type('iddqd');
  await page.getByRole('button', { name: 'Report an Issue' }).click();
  const description = page.locator('textarea[aria-label="Describe the Issue"]');
  await description.fill('Unsent issue draft');
  await openPalette(page);
  await expect(page.locator('.issue-report-popover')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(description).toHaveValue('Unsent issue draft');
  await expect(page.locator('.issue-report-popover')).toBeVisible();
  await page.keyboard.press('Escape');

  await page.getByRole('combobox', { name: 'Model' }).click();
  await expect(page.locator('.ui-select-filterable-list')).toBeVisible();
  await openPalette(page);
  await expect(
    page.getByRole('option', { name: /Switch Session/ }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.ui-select-filterable-list')).toBeVisible();
});

test('opens from the first-run titlebar launcher', async ({ page }) => {
  await page.goto('/?test-scenario=empty-workspace');

  await page.getByRole('button', { name: 'Open Command Palette' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Open Local Project/ }),
  ).toBeVisible();
  await page.getByRole('option', { name: /Open Project…/ }).click();
  await expect(page.getByRole('menuitem')).toHaveText([
    'Open Local Project',
    'Open Remote Project',
  ]);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Open Local Project' }).click();
});

test('selects model and thinking-effort options from the palette', async ({
  page,
}) => {
  // The browser sandbox is a full app fixture whose settings requests settle,
  // unlike the bootstrap scenario which intentionally stops after hydration.
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();

  await openPalette(page);
  await openNestedPage(page, 'Choose Model');
  await page.getByRole('option', { name: 'Tau Dev' }).click();
  await expect(page.getByRole('dialog', { name: 'Choose Model' })).toHaveCount(
    0,
  );
  await expect(page.getByRole('combobox', { name: 'Model' })).toHaveText(
    'Tau Dev',
  );
  await expect(composer).toBeFocused();

  await openPalette(page);
  await openNestedPage(page, 'Choose Thinking Effort');
  await page.getByRole('option', { name: 'High' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Choose Thinking Effort' }),
  ).toHaveCount(0);
  await expect(
    page.getByRole('combobox', { name: 'Thinking Effort' }),
  ).toHaveText('High');
  await expect(composer).toBeFocused();
});

test('opens the session page with Ctrl/Meta+P from the composer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.focus();
  await composer.evaluate((element) => {
    const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
    element.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'p',
        bubbles: true,
        cancelable: true,
        ...(isMac ? { metaKey: true } : { ctrlKey: true }),
      }),
    );
  });

  await expect(
    page.getByRole('dialog', { name: 'Switch Session' }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: 'Main Current' }),
  ).toBeVisible();
});

test('uses project identity and its location fallback when switching projects', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();

  await openPalette(page);
  await openNestedPage(page, 'Switch Project');
  const notes = page.getByRole('option', { name: /notes/ });
  await expect(notes.locator('.command-palette-title')).toHaveText('notes');
  await expect(notes.locator('.command-palette-detail')).toHaveText(
    '/browser-dev/projects/notes',
  );
  await notes.click();

  await expect(
    page.locator(
      '.project-group[data-project-path="/browser-dev/projects/notes"] .project-row',
    ),
  ).toHaveClass(/selected/);
  await expect(
    page.getByRole('heading', { name: 'Triage inbox' }),
  ).toBeVisible();
});

test('switching sessions from archived review returns to active sessions with the draft intact', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.fill('Keep this draft while reviewing archives');

  const navigation = page.locator('.session-row', {
    hasText: 'Navigation review',
  });
  await navigation.hover();
  await navigation
    .getByRole('button', { name: 'Archive Navigation review' })
    .click();
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  await expect(page.locator('.archived-list')).toBeVisible();
  await expect(page.locator('.archived-list')).toContainText(
    'Navigation review',
  );

  await openPalette(page);
  await openNestedPage(page, 'Switch Session');
  await page.getByRole('option', { name: 'Workspace overview' }).click();

  await expect(page.locator('.projects-body')).toBeVisible();
  await expect(page.locator('.archived-list')).toHaveCount(0);
  await expect(composer).toHaveValue(
    'Keep this draft while reviewing archives',
  );
});

test('a palette New Session targets its captured project, not the sidebar row that held focus', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await notes.locator('.project-toggle').focus();
  await openPalette(page);
  await expect(
    page
      .getByRole('option', { name: /Remove Project/ })
      .locator('.command-palette-detail'),
  ).toHaveText('atlas · /browser-dev/projects/atlas');
  await page.getByRole('option', { name: /New Session/ }).click();
  await expect(
    page.locator(
      '.project-group[data-project-path="/browser-dev/projects/atlas"] .project-row',
    ),
  ).toHaveClass(/selected/);
  await expect(notes.locator('.session-row')).toHaveCount(2);
});

test('a project-row New Session targets that row, not the active project', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await notes.hover();
  await notes.getByRole('button', { name: 'New Session in notes' }).click();
  await expect(notes.locator('.project-row')).toHaveClass(/selected/);
});

test('New Session shortcut uses a focused project row', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await notes.locator('.project-toggle').focus();
  await page.evaluate(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    await internals.invoke('plugin:event|emit', {
      event: 'tau://new-session',
      payload: undefined,
    });
  });
  await expect(notes.locator('.project-row')).toHaveClass(/selected/);
});

test('focused sidebar rows do not retarget composer and session cycling shortcuts', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  const noteSession = notes
    .locator('.session-row')
    .first()
    .locator('.session-select');
  await noteSession.focus();
  await page.keyboard.press(await modShortcut(page, 'Shift+m'));
  await expect(
    page.getByRole('dialog', { name: 'Choose Model' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await noteSession.focus();
  await page.keyboard.press('Control+Tab');
  await expect(
    page.getByRole('heading', { name: 'Navigation review' }),
  ).toBeVisible();
});

test('Cmd+Shift+R from the composer targets the active session, not a hovered row', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.focus();

  await page.locator('.session-row', { hasText: 'Navigation review' }).hover();
  await page.keyboard.press(await modShortcut(page, 'Shift+r'));

  const name = page.locator('input[aria-label="Session Name"]');
  await expect(name).toBeFocused();
  await expect(name).toHaveValue('Workspace overview');
  await name.fill('Renamed from composer');
  await name.press('Enter');
  await expect(
    page.getByRole('heading', { name: 'Renamed from composer' }),
  ).toBeVisible();
});

test('ignores a simulated native New Session event while the palette is open', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await openPalette(page);

  const eventMockAvailable = await page.evaluate(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__?: {
          invoke?: (cmd: string, args: unknown) => unknown;
        };
      }
    ).__TAURI_INTERNALS__;
    if (typeof internals?.invoke !== 'function') return false;
    await internals.invoke('plugin:event|emit', {
      event: 'tau://new-session',
      payload: undefined,
    });
    return true;
  });
  test.skip(
    !eventMockAvailable,
    'The browser mock does not expose app events.',
  );

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'New Session' })).toHaveCount(
    0,
  );
});
