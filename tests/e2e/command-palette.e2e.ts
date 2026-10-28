import type { Locator, Page } from '@playwright/test';

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

type SettingsPage = 'Choose Model' | 'Choose Thinking Effort';

async function openSettings(
  page: Page,
  title: SettingsPage,
  direct = true,
): Promise<Locator> {
  if (direct) {
    await page.keyboard.press(
      await modShortcut(page, title === 'Choose Model' ? 'Shift+m' : 'Shift+e'),
    );
  } else {
    await openPalette(page);
    await openNestedPage(page, title);
  }
  const search = page.getByRole('combobox', { name: title, exact: true });
  await expect(search).toBeFocused();
  await expect(search).toHaveValue('');
  return search;
}

async function expectCandidate(
  page: Page,
  search: Locator,
  name: string,
): Promise<void> {
  const row = page.getByRole('option', { name, exact: true });
  await expect(row).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('option', { selected: true })).toHaveCount(1);
  await expect(search).toHaveAttribute(
    'aria-activedescendant',
    (await row.getAttribute('id'))!,
  );
  await expect(row).toBeInViewport();
  await expect(search).toBeFocused();
}

async function loadSettingsScenario(
  page: Page,
  name = 'palette-settings',
): Promise<Locator> {
  await page.goto(`/?test-scenario=${name}`);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  return composer;
}

for (const [title, current, other, applied] of [
  ['Choose Model', 'Beta Shared', 'Alpha Shared', 'Model'],
  ['Choose Thinking Effort', 'High', 'Low', 'Thinking Effort'],
] as const) {
  test(`settings initial selection: ${title} uses the session value on every entry`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 360 });
    const composer = await loadSettingsScenario(page);
    await composer.fill('Preserve my settings draft');
    for (const direct of [true, false]) {
      const search = await openSettings(page, title, direct);
      await expectCandidate(page, search, current);
      await expect(
        page.getByRole('option', { name: other, exact: true }),
      ).toHaveAttribute('aria-selected', 'false');
      await search.press('ArrowUp');
      await expectCandidate(page, search, other);
      await search.press('ArrowUp');
      await expectCandidate(
        page,
        search,
        title === 'Choose Model' ? 'Gamma' : 'Off',
      );
      await page.getByRole('option', { name: other, exact: true }).hover();
      await expectCandidate(page, search, other);
      await page.mouse.move(0, 0);
      await search.press('Escape');
      await expect(
        page.getByRole('combobox', { name: applied, exact: true }),
      ).toHaveText(current);
      await expect(composer).toBeFocused();
      await expect(composer).toHaveValue('Preserve my settings draft');
    }
    const search = await openSettings(page, title, false);
    await expectCandidate(page, search, current);
    await page.getByRole('button', { name: 'Back', exact: true }).click();
    const root = page.getByRole('combobox', {
      name: 'Command Palette',
      exact: true,
    });
    await expect(root).toHaveValue(title);
    await expect(page.getByRole('option', { selected: true })).toContainText(
      title,
    );
    await root.press('Enter');
    await expectCandidate(page, search, current);
    await search.press('Backspace');
    await expect(root).toHaveValue(title);
    await root.press('Enter');
    await expectCandidate(page, search, current);
    await search.press('Escape');
  });

  test(`settings navigation: ${title} filters and moves candidates without applying`, async ({
    page,
  }) => {
    const composer = await loadSettingsScenario(page);
    await composer.fill('Preserve my navigation draft');
    const search = await openSettings(page, title);
    await expectCandidate(page, search, current);
    await search.fill(other);
    await expectCandidate(page, search, other);
    await search.fill('zzzz-no-setting');
    await expect(page.getByRole('option')).toHaveCount(0);
    await expect(search).not.toHaveAttribute('aria-activedescendant');
    await search.press('Enter');
    await expect(search).toBeVisible();
    await search.fill('');
    await expectCandidate(page, search, other);
    await search.press('Escape');
    await expect(composer).toBeFocused();
    await expect(composer).toHaveValue('Preserve my navigation draft');
    await expect(
      page.getByRole('combobox', { name: applied, exact: true }),
    ).toHaveText(current);
  });
}

test('settings confirmations use exact model provider and reopen with newly applied values', async ({
  page,
}) => {
  const composer = await loadSettingsScenario(page, 'palette-settings-confirm');
  await composer.fill('Keep draft through confirmations');
  let search = await openSettings(page, 'Choose Model');
  await search.fill('Alpha');
  await search.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Choose Model' })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole('combobox', { name: 'Model', exact: true }),
  ).toHaveText('Alpha Shared');
  await expect(composer).toBeFocused();
  search = await openSettings(page, 'Choose Model');
  await expectCandidate(page, search, 'Alpha Shared');
  await search.press('Escape');
  await openSettings(page, 'Choose Thinking Effort', false);
  await page.getByRole('option', { name: 'Off', exact: true }).click();
  await expect(
    page.getByRole('dialog', { name: 'Choose Thinking Effort' }),
  ).toHaveCount(0);
  await expect(
    page.getByRole('combobox', { name: 'Thinking Effort', exact: true }),
  ).toHaveText('Off');
  await expect(composer).toBeFocused();
  search = await openSettings(page, 'Choose Thinking Effort');
  await expectCandidate(page, search, 'Off');
  await search.press('Escape');
  await expect(composer).toHaveValue('Keep draft through confirmations');
});

test('settings initial selection: session switches isolate model, effort, and drafts', async ({
  page,
}) => {
  const composer = await loadSettingsScenario(
    page,
    'palette-settings-sessions',
  );
  await composer.fill('Main draft survives');
  for (const [index, [session, model, effort]] of (
    [
      ['Main', 'Beta Shared', 'High'],
      ['Backup', 'Gamma', 'Off'],
      ['Main', 'Beta Shared', 'High'],
    ] as const
  ).entries()) {
    if (index > 0) {
      await page
        .locator('.session-row', { hasText: session })
        .locator('.session-select')
        .click();
      await expect(
        page.getByRole('heading', { name: session, exact: true }),
      ).toBeVisible();
      await expect(composer).toBeEnabled();
    }
    for (const [title, candidate] of [
      ['Choose Model', model],
      ['Choose Thinking Effort', effort],
    ] as const) {
      const search = await openSettings(page, title);
      await expectCandidate(page, search, candidate);
      await search.press('ArrowDown');
      await search.press('Escape');
    }
    if (session === 'Backup') await expect(composer).toHaveValue('');
    else await expect(composer).toHaveValue('Main draft survives');
  }
});

test('keeps the palette inside short windows', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  for (const height of [800, 360]) {
    await page.setViewportSize({ width: 1280, height });
    const bounds = await page
      .getByRole('dialog', { name: 'Command Palette' })
      .boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(height);
  }

  await expect(
    page.getByRole('combobox', { name: 'Command Palette' }),
  ).toBeVisible();
});

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

test('traverses session groups with the keyboard and preserves drafts when switching projects', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.fill('Keep this atlas draft');
  await openPalette(page);
  await openNestedPage(page, 'Switch Session');

  const picker = page.getByRole('dialog', { name: 'Switch Session' });
  const search = picker.getByRole('combobox', { name: 'Switch Session' });
  await expect(
    picker.getByRole('option', { name: 'Workspace overview Current' }),
  ).toHaveAttribute('aria-selected', 'true');
  await search.press('ArrowDown');
  await expect(
    picker.getByRole('option', { name: 'Navigation review' }),
  ).toHaveAttribute('aria-selected', 'true');
  await search.press('ArrowDown');
  await expect(
    picker.getByRole('option', { name: 'Release checklist' }),
  ).toHaveAttribute('aria-selected', 'true');
  await search.press('ArrowDown');
  const triage = picker.getByRole('option', { name: 'Triage inbox' });
  await expect(triage).toHaveAttribute('aria-selected', 'true');
  await search.press('Enter');
  await expect(
    page.getByRole('heading', { name: 'Triage inbox' }),
  ).toBeVisible();

  await openPalette(page);
  await openNestedPage(page, 'Switch Session');
  await expect(
    page
      .getByRole('dialog', { name: 'Switch Session' })
      .getByRole('option', { name: /Triage inbox Current/ }),
  ).toHaveAttribute('aria-selected', 'true');
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name: /Workspace overview/ })
    .click();
  await expect(
    page.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();
  await expect(composer).toHaveValue('Keep this atlas draft');
});

for (const destination of ['Composer', 'Sidebar', 'Transcript'] as const) {
  test(`palette-only Focus ${destination} keeps its destination`, async ({
    page,
  }) => {
    await page.goto('/');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    const sidebar = page
      .locator(
        '.session-row.selected .session-select, .project-row.selected .project-select',
      )
      .first();
    const transcript = page.getByLabel('Transcript', { exact: true });
    await expect(composer).toBeEnabled();
    await composer.fill('Keep the focus draft');
    const origin = destination === 'Composer' ? sidebar : composer;
    const target =
      destination === 'Composer'
        ? composer
        : destination === 'Sidebar'
          ? sidebar
          : transcript;
    await origin.focus();
    await openPalette(page);
    await openNestedPage(page, `Focus ${destination}`);
    await expect(
      page.getByRole('dialog', { name: 'Command Palette' }),
    ).toHaveCount(0);
    await expect(target).toBeFocused();
    await expect(composer).toHaveValue('Keep the focus draft');
  });
}

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

test('clicking the header palette preserves an uncommitted session rename', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  await page.getByRole('button', { name: 'Main', exact: true }).click();
  const name = page.locator('input[aria-label="Session Name"]');
  await name.fill('Uncommitted click rename');
  await page
    .locator('.session-header')
    .getByRole('button', { name: 'Open Command Palette' })
    .click();

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await expect(
    page.getByRole('combobox', { name: 'Command Palette' }),
  ).toBeFocused();
  await expect(name).toHaveValue('Uncommitted click rename');
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(name).toHaveValue('Uncommitted click rename');
  await expect(name).toBeFocused();
  await expect(
    page.getByRole('heading', { name: 'Uncommitted click rename' }),
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

test('recognizes a Mac Option-produced character for Open Remote Project', async ({
  page,
}) => {
  test.skip(
    !(await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform))),
    'Mac Option layout only',
  );
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page.evaluate(() => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'ø',
        code: 'KeyO',
        metaKey: true,
        altKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
  });
  await expect(
    page.getByRole('dialog', { name: 'SSH Connection' }),
  ).toBeVisible();
});

test('project switcher selects the nonfirst active project, including when collapsed', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 360 });
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  const notesGroup = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await notesGroup.locator('.session-select').first().click();
  await expect(
    page.getByRole('heading', { name: 'Triage inbox' }),
  ).toBeVisible();
  await composer.fill('Keep this notes draft');
  await notesGroup.locator('.project-toggle').click();
  await expect(notesGroup.locator('.session-row')).toHaveCount(0);

  const expectSelectedProject = async (
    name: string,
    search: Locator,
  ): Promise<void> => {
    const row = page.locator('.command-palette-row').filter({
      has: page.locator('.command-palette-title', { hasText: name }),
    });
    await expect(row).toHaveAttribute('aria-selected', 'true');
    await expect(row).toBeInViewport();
    await expect(search).toHaveAttribute(
      'aria-activedescendant',
      (await row.getAttribute('id'))!,
    );
  };

  await page.keyboard.press(await modShortcut(page, 'Shift+p'));
  const direct = page.getByRole('dialog', { name: 'Switch Project' });
  const directSearch = direct.getByRole('combobox', { name: 'Switch Project' });
  await expect(directSearch).toBeFocused();
  await expect(directSearch).toHaveValue('');
  await expect(
    direct.locator('.command-palette-title').allTextContents(),
  ).resolves.toEqual(['atlas', 'notes']);
  await expectSelectedProject('notes', directSearch);
  await directSearch.press('Enter');
  await expect(
    page.getByRole('heading', { name: 'Triage inbox' }),
  ).toBeVisible();
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('Keep this notes draft');

  await page.keyboard.press(await modShortcut(page, 'Shift+p'));
  await expect(directSearch).toBeFocused();
  await expectSelectedProject('notes', directSearch);
  await directSearch.fill('no matching project');
  await expect(direct.getByRole('option')).toHaveCount(0);
  await expect(directSearch).not.toHaveAttribute('aria-activedescendant');
  await directSearch.fill('');
  await expectSelectedProject('atlas', directSearch);
  await directSearch.press('Escape');

  await openPalette(page);
  await openNestedPage(page, 'Switch Project');
  const nested = page.getByRole('dialog', { name: 'Switch Project' });
  const nestedSearch = nested.getByRole('combobox', { name: 'Switch Project' });
  await expect(nestedSearch).toBeFocused();
  await expect(nestedSearch).toHaveValue('');
  await expectSelectedProject('notes', nestedSearch);
  await nestedSearch.fill('atlas');
  await expectSelectedProject('atlas', nestedSearch);
  await nestedSearch.press('Enter');
  await expect(
    page.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();

  await page.keyboard.press(await modShortcut(page, 'Shift+p'));
  await expect(directSearch).toBeFocused();
  await expectSelectedProject('atlas', directSearch);
  await directSearch.press('Escape');
});

test('palette actions return focus to an editable destination, including the current session', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await openPalette(page);
  await openNestedPage(page, 'Switch Session');
  await page.getByRole('option', { name: 'Workspace overview' }).click();
  await expect(composer).toBeFocused();

  await openPalette(page);
  await page.getByRole('option', { name: /Mark as Unread/ }).click();
  await expect(composer).toBeFocused();
});

test('archived-row shortcuts use the focused cross-project identity, not the active transcript', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  const note = notes.locator('.session-row').first();
  const title = await note.locator('.session-title').textContent();
  expect(title).toBeTruthy();
  await note.hover();
  await note.getByRole('button', { name: /Archive/ }).click();
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  const archived = page
    .locator('.archived-list .row')
    .filter({ hasText: title! });
  await archived.getByRole('button', { name: `Open ${title}` }).focus();
  await page.keyboard.press(await modShortcut(page, 'Shift+a'));
  await expect(archived).toHaveCount(0);
  await expect(
    page.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Show Sessions' }).click();
  const restored = notes.locator('.session-row').filter({ hasText: title! });
  await expect(restored).toBeVisible();
  await restored.locator('.session-select').focus();
  await restored.evaluate((row) => {
    row.dataset.sessionId = 'stale-session-id';
  });
  await page.keyboard.press(await modShortcut(page, 'Shift+a'));
  await expect(
    page.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();
  await expect(restored).toBeVisible();
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
  await notes.locator('.project-select').focus();
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
  await notes.locator('.project-select').click({ button: 'right' });
  await page
    .getByRole('menuitem', { name: 'New Session', exact: true })
    .click();
  await expect(notes.locator('.project-row')).toHaveClass(/selected/);
});

test('New Session shortcut uses a focused project row', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const notes = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await notes.locator('.project-select').focus();
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

test('focused sidebar rows do not retarget composer shortcuts and palette session cycling', async ({
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
  await openPalette(page);
  await openNestedPage(page, 'Next Session');
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

test('palette keyboard navigation wraps and reveals the selected row after manual scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 360 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  const results = page.locator('.command-palette-results');
  await results.evaluate((element) => {
    element.scrollTop = 0;
  });
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await search.press('ArrowUp');
  const selected = results.locator('.command-palette-row.selected');
  const selectedId = await selected.getAttribute('id');
  expect(selectedId).not.toBeNull();
  await expect(selected).toBeInViewport();
  await expect(search).toHaveAttribute('aria-activedescendant', selectedId!);
  await expect(search).toBeFocused();
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
