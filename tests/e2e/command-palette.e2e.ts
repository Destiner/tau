import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap';

type ButtonAppearance = {
  background: string;
  color: string;
  opacity: string;
  bounds: { x: number; y: number; width: number; height: number };
};

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

test('places the palette 15% down and keeps it inside short windows', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  for (const height of [800, 360]) {
    await page.setViewportSize({ width: 1280, height });
    const bounds = await page
      .getByRole('dialog', { name: 'Command Palette' })
      .boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.y / height).toBeCloseTo(0.15, 2);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(height - 31);
  }

  await expect(
    page.getByRole('combobox', { name: 'Command Palette' }),
  ).toBeVisible();
});

async function expectPaletteRowLayout(
  row: Locator,
  { titleClipped = false, detailClipped = false } = {},
): Promise<void> {
  const geometry = await row.evaluate(async (element) => {
    await document.fonts.ready;
    const results = element.parentElement!;
    const title = element.querySelector<HTMLElement>('.command-palette-title')!;
    const detail = element.querySelector<HTMLElement>(
      '.command-palette-detail',
    );
    const shortcut = element.querySelector<HTMLElement>('kbd');
    const rowBounds = element.getBoundingClientRect();
    const titleBounds = title.getBoundingClientRect();
    const detailBounds = detail?.getBoundingClientRect();
    const shortcutBounds = shortcut?.getBoundingClientRect();
    return {
      titleClipped: title.scrollWidth - title.clientWidth,
      detailClipped: detail ? detail.scrollWidth - detail.clientWidth : 0,
      titleRight: titleBounds.right,
      detailLeft: detailBounds?.left,
      detailRight: detailBounds?.right,
      shortcutLeft: shortcutBounds?.left,
      shortcutRight: shortcutBounds?.right,
      rowRight: rowBounds.right,
      height: rowBounds.height,
      listOverflow: results.scrollWidth - results.clientWidth,
    };
  });
  if (titleClipped) expect(geometry.titleClipped).toBeGreaterThan(1);
  else expect(geometry.titleClipped).toBeLessThanOrEqual(1);
  if (detailClipped) expect(geometry.detailClipped).toBeGreaterThan(1);
  expect(geometry.height).toBe(28);
  expect(geometry.listOverflow).toBeLessThanOrEqual(1);
  if (geometry.detailLeft !== undefined) {
    expect(geometry.titleRight).toBeLessThanOrEqual(geometry.detailLeft + 1);
    expect(geometry.detailRight!).toBeLessThanOrEqual(
      (geometry.shortcutLeft ?? geometry.rowRight) + 1,
    );
  }
  if (geometry.shortcutLeft !== undefined) {
    expect(geometry.titleRight).toBeLessThanOrEqual(geometry.shortcutLeft + 1);
    expect(geometry.shortcutRight!).toBeLessThanOrEqual(geometry.rowRight + 1);
  }
}

async function openNestedPage(page: Page, command: string): Promise<void> {
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await search.fill(command);
  await search.press('Enter');
}

test('prioritizes commands over long session details at desktop and narrow widths', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await page
    .getByRole('button', { name: 'Workspace overview', exact: true })
    .click();
  const name = page.locator('input[aria-label="Session Name"]');
  await name.fill(
    'A long session name with many words that cannot fit beside any command label in the palette',
  );
  await name.press('Enter');
  await expect(
    page.getByRole('heading', { name: /A long session name/ }),
  ).toBeVisible();

  await openPalette(page);
  await expectPaletteRowLayout(
    page.getByRole('option', { name: /Mark as Unread/ }),
    { detailClipped: true },
  );
  await expectPaletteRowLayout(
    page.getByRole('option', { name: /Archive Session/ }),
    { detailClipped: true },
  );
  await page.getByRole('option', { name: /Mark as Unread/ }).click();
  await expect(composer).toBeFocused();

  await page.setViewportSize({ width: 360, height: 800 });
  await openPalette(page);
  await expectPaletteRowLayout(
    page.getByRole('option', { name: /Mark as Read/ }),
    { detailClipped: true },
  );
});

test('keeps project identities ahead of locations and bounds oversized session names', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page
    .getByRole('button', { name: 'Workspace overview', exact: true })
    .click();
  const name = page.locator('input[aria-label="Session Name"]');
  await name.fill(
    'An exceptionally long primary session name that must ellipsize within a single compact picker row even when the window is narrow',
  );
  await name.press('Enter');

  await page.setViewportSize({ width: 260, height: 800 });
  await openPalette(page);
  await expectPaletteRowLayout(
    page.getByRole('option', { name: /Remove Project/ }),
    {
      detailClipped: true,
    },
  );
  await page.setViewportSize({ width: 180, height: 800 });
  await openNestedPage(page, 'Switch Project');
  await expectPaletteRowLayout(page.getByRole('option', { name: /atlas/ }), {
    detailClipped: true,
  });
  await page.setViewportSize({ width: 260, height: 800 });
  await page.getByRole('button', { name: 'Back' }).click();
  await openNestedPage(page, 'Switch Session');
  await expectPaletteRowLayout(
    page.getByRole('option', { name: /An exceptionally long/ }),
    {
      titleClipped: true,
    },
  );
});

test('aligns command detail baselines without shifting rows or shortcuts', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openPalette(page);

  const geometry = await page
    .getByRole('option', { name: /Archive Session/ })
    .evaluate(async (row) => {
      await document.fonts.ready;
      const title = row.querySelector('.command-palette-title')!;
      const detail = row.querySelector('.command-palette-detail')!;
      const baseline = (text: Element): number => {
        const probe = document.createElement('span');
        probe.style.cssText =
          'display:inline-block;width:0;height:0;vertical-align:baseline';
        text.prepend(probe);
        const y = probe.getBoundingClientRect().bottom;
        probe.remove();
        return y;
      };
      const offset = baseline(detail) - baseline(title);
      const bounds = row.getBoundingClientRect();
      const shortcut = row.querySelector('kbd')!.getBoundingClientRect();
      return {
        offset,
        height: bounds.height,
        shortcutInside:
          shortcut.left >= title.getBoundingClientRect().right &&
          shortcut.right <= bounds.right,
        shortcutCenterOffset:
          shortcut.top + shortcut.height / 2 - (bounds.top + bounds.height / 2),
      };
    });

  expect(Math.abs(geometry.offset)).toBeLessThan(0.2);
  expect(geometry.height).toBe(28);
  expect(geometry.shortcutInside).toBe(true);
  expect(Math.abs(geometry.shortcutCenterOffset)).toBeLessThan(0.2);
  expect(
    await page
      .getByRole('option', { name: /Choose Model/ })
      .evaluate((row) => row.getBoundingClientRect().height),
  ).toBe(geometry.height);
});

for (const colorScheme of ['light', 'dark'] as const) {
  test(`session header actions share fade feedback in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await page.goto(scenarioUrl);

    const header = page.locator('.session-header');
    const newSession = header.getByRole('button', { name: 'New Session' });
    const palette = header.getByRole('button', {
      name: 'Open Command Palette',
    });
    await expect(newSession).toBeEnabled();
    await expect(palette).toBeEnabled();
    await page.mouse.move(0, 100);

    const appearance = async (button: Locator): Promise<ButtonAppearance> =>
      button.evaluate((element) => {
        const style = getComputedStyle(element);
        const { x, y, width, height } = element.getBoundingClientRect();
        return {
          background: style.backgroundColor,
          color: style.color,
          opacity: style.opacity,
          bounds: { x, y, width, height },
        };
      });

    const idleNew = await appearance(newSession);
    const idlePalette = await appearance(palette);
    expect(idleNew.background).toBe('rgba(0, 0, 0, 0)');
    expect(idlePalette.background).toBe(idleNew.background);
    expect(idlePalette.color).toBe(idleNew.color);
    expect(idlePalette.opacity).toBe(idleNew.opacity);
    expect(idlePalette.bounds.y).toBe(idleNew.bounds.y);
    expect(idlePalette.bounds.width).toBe(idleNew.bounds.width);
    expect(idlePalette.bounds.height).toBe(idleNew.bounds.height);

    await newSession.hover();
    const hoveredNew = await appearance(newSession);
    expect(hoveredNew.background).toBe(idleNew.background);
    expect(hoveredNew.opacity).toBe('1');
    expect(hoveredNew.bounds).toEqual(idleNew.bounds);

    await palette.hover();
    const hoveredPalette = await appearance(palette);
    expect(hoveredPalette.background).toBe(idleNew.background);
    expect(hoveredPalette.color).toBe(hoveredNew.color);
    expect(hoveredPalette.opacity).toBe(hoveredNew.opacity);
    expect(hoveredPalette.bounds).toEqual(idlePalette.bounds);
  });
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

test('opens from the first-run titlebar launcher without update commands', async ({
  page,
}) => {
  await page.goto('/?test-scenario=empty-workspace');

  await page.getByRole('button', { name: 'Open Command Palette' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: /Open Local Project/ }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: 'Show Update Status' }),
  ).toHaveCount(0);
  await expect(
    page.getByRole('option', { name: 'Check for Updates' }),
  ).toHaveCount(0);
  await page.getByRole('option', { name: /Open Project…/ }).click();
  await expect(page.getByRole('menuitem')).toHaveText([
    'Open Local Project',
    'Open Remote Project',
  ]);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Open Local Project' }).click();
});

test('shows update commands in a project palette', async ({ page }) => {
  await page.goto(scenarioUrl);

  await openPalette(page);
  await expect(
    page.getByRole('option', { name: 'Show Update Status' }),
  ).toBeVisible();
  await expect(
    page.getByRole('option', { name: 'Check for Updates' }),
  ).toBeVisible();
});

test('keeps Report an Issue in the palette without a keyboard shortcut', async ({
  page,
}) => {
  await page.goto(scenarioUrl);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await page.getByLabel('Projects and Sessions').click();
  await page.keyboard.type('iddqd');
  const reporter = page.getByRole('dialog', { name: 'Report an Issue' });
  await page.keyboard.press(await modShortcut(page, 'Shift+i'));
  await expect(reporter).toHaveCount(0);

  await openPalette(page);
  const command = page.getByRole('option', { name: 'Report an Issue' });
  await expect(command.locator('kbd')).toHaveCount(0);
  await command.click();
  await expect(reporter).toBeVisible();
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
  const notesGroup = page.locator(
    '.project-group[data-project-path="/browser-dev/projects/notes"]',
  );
  await page.keyboard.press('Escape');
  await notesGroup.locator('.project-toggle').click();
  await expect(notesGroup.locator('.session-row')).toHaveCount(0);
  await openPalette(page);
  await openNestedPage(page, 'Switch Project');
  await notes.click();

  await expect(notesGroup.locator('.project-row')).toHaveClass(/selected/);
  await expect(notesGroup.locator('.session-row').first()).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Triage inbox' }),
  ).toBeVisible();
});

test('shows shared shortcuts for state-dependent palette and composer commands', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  const mod = (await modShortcut(page, 'a')).startsWith('Meta') ? '⌘' : 'Ctrl+';
  await openPalette(page);
  await expect(
    page.getByRole('option', { name: /Archive Session/ }).locator('kbd'),
  ).toHaveText(`${mod}⇧A`.replace('Ctrl+⇧', 'Ctrl+Shift+'));
  await expect(
    page.getByRole('option', { name: /Mark as Unread/ }).locator('kbd'),
  ).toHaveText(`${mod}⇧U`.replace('Ctrl+⇧', 'Ctrl+Shift+'));
  await page.getByRole('option', { name: /Mark as Unread/ }).click();
  await openPalette(page);
  await expect(
    page.getByRole('option', { name: /Mark as Read/ }).locator('kbd'),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await composer.fill('Discover send');
  await page.getByRole('button', { name: 'Send Message' }).focus();
  await expect(
    page.locator('.ui-tooltip', { hasText: 'Send Message' }).locator('kbd'),
  ).toHaveText('Enter');
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

async function nextRenderFrame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => requestAnimationFrame(() => resolve())),
  );
}

async function paletteScrollTop(page: Page): Promise<number> {
  return page
    .locator('.command-palette-results')
    .evaluate((results) => results.scrollTop);
}

test('palette hover highlights a clipped row without moving its native scroll position', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 360 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  const results = page.locator('.command-palette-results');
  expect(
    await results.evaluate(
      (element) => element.scrollHeight > element.clientHeight,
    ),
  ).toBe(true);
  await nextRenderFrame(page);
  await page.mouse.move(0, 0);
  await results.evaluate((element) => {
    const row = element.querySelector<HTMLElement>(
      '.command-palette-row:nth-child(2)',
    );
    if (!row) throw new Error('Expected a second palette command.');
    element.scrollTop +=
      row.getBoundingClientRect().top - element.getBoundingClientRect().top + 8;
  });

  const before = await paletteScrollTop(page);
  const row = results.locator('.command-palette-row:nth-child(2)');
  const rowBounds = await row.boundingBox();
  const resultsBounds = await results.boundingBox();
  expect(rowBounds).not.toBeNull();
  expect(resultsBounds).not.toBeNull();
  const visibleTop = Math.max(resultsBounds!.y, rowBounds!.y);
  const visibleBottom = Math.min(
    resultsBounds!.y + resultsBounds!.height,
    rowBounds!.y + rowBounds!.height,
  );
  expect(visibleBottom).toBeGreaterThan(visibleTop);
  await page.mouse.move(rowBounds!.x + 12, (visibleTop + visibleBottom) / 2);
  await nextRenderFrame(page);

  await expect(row).toHaveAttribute('aria-selected', 'true');
  expect(Math.abs((await paletteScrollTop(page)) - before)).toBeLessThanOrEqual(
    1,
  );
  await expect(
    page.getByRole('combobox', { name: 'Command Palette' }),
  ).toBeFocused();
});

test('palette wheel scrolling stays native at both pointer edges', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 360 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  const results = page.locator('.command-palette-results');
  await results.evaluate((element) => {
    element.scrollTop = 80;
  });
  const bounds = await results.boundingBox();
  expect(bounds).not.toBeNull();
  await page.mouse.move(bounds!.x + 20, bounds!.y + bounds!.height - 3);
  const beforeDown = await paletteScrollTop(page);
  await page.mouse.wheel(0, 13);
  await nextRenderFrame(page);
  const afterDown = await paletteScrollTop(page);
  expect(afterDown).toBeGreaterThan(beforeDown);
  expect(afterDown - beforeDown).toBeLessThanOrEqual(20);

  await page.mouse.move(bounds!.x + 20, bounds!.y + 3);
  await nextRenderFrame(page);
  const beforeUp = await paletteScrollTop(page);
  await page.mouse.wheel(0, -13);
  await nextRenderFrame(page);
  const afterUp = await paletteScrollTop(page);
  expect(afterUp).toBeLessThan(beforeUp);
  expect(beforeUp - afterUp).toBeLessThanOrEqual(20);
  await expect(
    page.getByRole('combobox', { name: 'Command Palette' }),
  ).toBeFocused();
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

test('palette filtering restores a valid visible selection after manual scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 360 });
  await page.goto(scenarioUrl);
  await openPalette(page);

  const results = page.locator('.command-palette-results');
  const search = page.getByRole('combobox', { name: 'Command Palette' });
  await results.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await search.fill('switch project');
  const filtered = results.locator('.command-palette-row.selected');
  await expect(filtered).toHaveCount(1);
  await expect(filtered).toHaveAttribute('aria-selected', 'true');
  expect(
    await filtered.evaluate((row) => {
      const list = row.parentElement!;
      const rowBounds = row.getBoundingClientRect();
      const listBounds = list.getBoundingClientRect();
      return (
        rowBounds.top >= listBounds.top && rowBounds.bottom <= listBounds.bottom
      );
    }),
  ).toBe(true);

  await search.fill('');
  const restored = results.locator('.command-palette-row.selected');
  await expect(restored).toHaveCount(1);
  expect(
    await restored.evaluate((row) => {
      const list = row.parentElement!;
      const rowBounds = row.getBoundingClientRect();
      const listBounds = list.getBoundingClientRect();
      return (
        rowBounds.top >= listBounds.top && rowBounds.bottom <= listBounds.bottom
      );
    }),
  ).toBe(true);
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
