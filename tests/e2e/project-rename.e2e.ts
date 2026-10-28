import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const root = '/browser-dev/projects';
const atlasPath = `${root}/atlas`;
const notesPath = `${root}/notes`;

type RenameMode = 'pass' | 'fail' | 'defer' | 'defer-fail';

declare global {
  interface Window {
    __tauRenameMock?: {
      calls: { path: string; name: string }[];
      mode: RenameMode;
      remoteCalls: string[];
      settled: number;
      release: () => void;
    };
  }
}

function project(page: Page, path = atlasPath): Locator {
  return page.locator(`.project-group[data-project-path="${path}"]`);
}

function dialog(page: Page): Locator {
  return page.getByRole('dialog', { name: 'Rename Project', exact: true });
}

async function shortcut(page: Page, key: string): Promise<string> {
  return (await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform)))
    ? `Meta+${key}`
    : `Control+${key}`;
}

async function openPalette(page: Page): Promise<void> {
  await page.keyboard.press(await shortcut(page, 'k'));
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toBeVisible();
}

async function openRename(
  page: Page,
  path = atlasPath,
  expectFocused = true,
): Promise<void> {
  await project(page, path)
    .locator('.project-select')
    .click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Rename Project…' }).click();
  await expect(dialog(page)).toBeVisible();
  if (expectFocused)
    await expect(
      dialog(page).getByRole('textbox', { name: 'Project Name' }),
    ).toBeFocused();
}

async function calls(page: Page): Promise<{ path: string; name: string }[]> {
  return page.evaluate(() => window.__tauRenameMock!.calls);
}

async function setMode(page: Page, mode: RenameMode): Promise<void> {
  await page.evaluate((value) => {
    window.__tauRenameMock!.mode = value;
  }, mode);
}

async function installRenameStore(
  page: Page,
  archived = false,
  remote = false,
): Promise<void> {
  // Persist only names across reloads; the sandbox remains the source of session/runtime data.
  await page.addInitScript(
    ({ withArchive, withRemote }) => {
      type Internals = {
        invoke?: (command: string, args?: unknown) => Promise<unknown>;
      };
      const tauWindow = window as typeof window & {
        __TAURI_INTERNALS__?: Internals;
      };
      let internals: Internals | undefined;
      const storageKey = 'tau-e2e-project-rename';
      window.__tauRenameMock = {
        calls: [],
        mode: 'pass',
        remoteCalls: [],
        settled: 0,
        release: (): void => {},
      };
      Object.defineProperty(tauWindow, '__TAURI_INTERNALS__', {
        configurable: true,
        get: () => internals,
        set: (next: Internals): void => {
          internals = next;
          let invoke: Internals['invoke'];
          Object.defineProperty(next, 'invoke', {
            configurable: true,
            get: () => invoke,
            set: (nextInvoke: NonNullable<Internals['invoke']>): void => {
              invoke = async (command, args): Promise<unknown> => {
                if (command === 'rename_project') {
                  const request = args as { path: string; name: string };
                  const mock = window.__tauRenameMock!;
                  mock.calls.push({ path: request.path, name: request.name });
                  const mode = mock.mode;
                  if (mode === 'defer' || mode === 'defer-fail')
                    await new Promise<void>((resolve) => {
                      mock.release = resolve;
                    });
                  if (mode === 'fail' || mode === 'defer-fail') {
                    mock.settled += 1;
                    throw new Error('Private registry failure');
                  }
                  const saved = await nextInvoke(command, args);
                  const names = JSON.parse(
                    localStorage.getItem(storageKey) ?? '{}',
                  ) as Record<string, string>;
                  names[request.path] = saved as string;
                  localStorage.setItem(storageKey, JSON.stringify(names));
                  mock.settled += 1;
                  return saved;
                }
                if (command.includes('ssh') || command.includes('remote'))
                  window.__tauRenameMock!.remoteCalls.push(command);
                const result = await nextInvoke(command, args);
                if (command !== 'load_workspace') return result;
                const workspace = structuredClone(result) as {
                  projects: {
                    path: string;
                    name: string;
                    connectionString?: string;
                    workingDirectory?: string;
                    sessions: { archived: boolean; selected: boolean }[];
                  }[];
                };
                const names = JSON.parse(
                  localStorage.getItem(storageKey) ?? '{}',
                ) as Record<string, string>;
                for (const item of workspace.projects)
                  item.name = names[item.path] ?? item.name;
                if (withRemote) {
                  const remoteProject = workspace.projects.find(
                    (item) => item.path === '/browser-dev/projects/notes',
                  )!;
                  remoteProject.connectionString = 'ssh tau@buildbox';
                  remoteProject.workingDirectory = '/srv/notes';
                }
                if (withArchive) {
                  const session = workspace.projects[0]!.sessions.find(
                    (item) => !item.selected,
                  )!;
                  session.archived = true;
                }
                return workspace;
              };
            },
          });
        },
      });
    },
    { withArchive: archived, withRemote: remote },
  );
}

test.beforeEach(async ({ page }, testInfo) => {
  await installRenameStore(
    page,
    testInfo.title.startsWith('archived session'),
    testInfo.title.startsWith('remote project'),
  );
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
});

test('inactive project context menu does not change selection and selects the prefilled name', async ({
  page,
}) => {
  const selected = page.locator('.session-row.selected');
  const selection = await selected.getAttribute('data-session-id');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Keep the active session draft');
  await openRename(page, notesPath);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await expect(input).toHaveValue('notes');
  expect(
    await input.evaluate((element: HTMLInputElement) => [
      element.selectionStart,
      element.selectionEnd,
    ]),
  ).toEqual([0, 5]);
  await expect(selected).toHaveAttribute('data-session-id', selection!);
  await expect(page.locator('textarea[aria-label="Message Pi"]')).toHaveValue(
    'Keep the active session draft',
  );
  expect(await calls(page)).toEqual([]);
  await dialog(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await expect(
    project(page, notesPath).locator('.project-select'),
  ).toBeFocused();
});

test('Cancel and Escape preserve rename and composer drafts on reopen and restore focus', async ({
  page,
}) => {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Unsent composer draft');
  for (const dismiss of ['cancel', 'escape'] as const) {
    await openRename(page);
    await dialog(page)
      .getByRole('textbox', { name: 'Project Name' })
      .fill('Unfinished name');
    if (dismiss === 'cancel')
      await dialog(page)
        .getByRole('button', { name: 'Cancel', exact: true })
        .click();
    else await page.keyboard.press('Escape');
    await expect(dialog(page)).toHaveCount(0);
    await expect(project(page).locator('.project-select')).toBeFocused();
    await expect(composer).toHaveValue('Unsent composer draft');
    await expect(project(page).locator('.project-select')).toHaveText('atlas');
    await openRename(page);
    await expect(
      dialog(page).getByRole('textbox', { name: 'Project Name' }),
    ).toHaveValue('Unfinished name');
    await page.keyboard.press('Escape');
  }
  expect(await calls(page)).toEqual([]);
});

test('blank Enter validates inline; IME Enter is ignored; real Enter saves a trimmed name', async ({
  page,
}) => {
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('   ');
  await input.press('Enter');
  await expect(dialog(page).getByRole('alert')).toHaveText(
    'Enter a project name.',
  );
  await expect(input).toHaveAttribute('aria-invalid', 'true');
  await expect(input).toBeFocused();
  expect(await calls(page)).toEqual([]);
  await input.fill('  Atlas Renamed  ');
  for (const composition of [{ isComposing: true }, { keyCode: 229 }]) {
    const prevented = await input.evaluate((element, properties) => {
      const event = new KeyboardEvent('keydown', {
        key: 'Enter',
        bubbles: true,
        cancelable: true,
        ...properties,
      });
      element.dispatchEvent(event);
      return event.defaultPrevented;
    }, composition);
    expect(prevented).toBe(false);
    await expect(dialog(page)).toBeVisible();
    expect(await calls(page)).toEqual([]);
  }
  await input.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toHaveText(
    'Atlas Renamed',
  );
  expect(await calls(page)).toEqual([
    { path: atlasPath, name: 'Atlas Renamed' },
  ]);
});

test('keeps Tab and Shift+Tab contained in the rename dialog', async ({
  page,
  browserName,
}) => {
  test.skip(
    browserName === 'webkit',
    'WebKit follows the host full-keyboard-access preference for Tab traversal.',
  );
  await openRename(page);
  const rename = dialog(page);
  const input = rename.getByRole('textbox', { name: 'Project Name' });
  const cancel = rename.getByRole('button', { name: 'Cancel', exact: true });
  const confirm = rename.getByRole('button', { name: 'Rename', exact: true });

  await expect(input).toBeFocused();
  for (const key of ['Tab', 'Tab', 'Tab', 'Tab', 'Shift+Tab']) {
    await page.keyboard.press(key);
    expect(
      await rename.evaluate((element) =>
        element.contains(document.activeElement),
      ),
    ).toBe(true);
  }
  await expect(cancel).toBeVisible();
  await expect(confirm).toBeVisible();
});

test('failure rolls back the label, keeps the draft inline, and supports retry', async ({
  page,
}) => {
  await setMode(page, 'fail');
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Atlas Retry');
  await dialog(page)
    .getByRole('button', { name: 'Rename', exact: true })
    .click();
  await expect(dialog(page).getByRole('alert')).toHaveText(
    'The project could not be renamed. Try again.',
  );
  await expect(dialog(page)).not.toContainText('Private registry failure');
  await expect(project(page).locator('.project-select')).toHaveText('atlas');
  await expect(input).toHaveValue('Atlas Retry');
  await expect(input).toBeEnabled();
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
  await setMode(page, 'pass');
  await dialog(page)
    .getByRole('button', { name: 'Retry', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toHaveText(
    'Atlas Retry',
  );
  expect(await calls(page)).toEqual(
    Array.from({ length: 2 }, () => ({ path: atlasPath, name: 'Atlas Retry' })),
  );
});

test('rename failure repairs the input disabled by submission without reselecting its draft', async ({
  page,
}) => {
  await setMode(page, 'defer-fail');
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Atlas Pending');
  await input.press('Enter');
  await expect(input).toBeDisabled();
  await page.evaluate(() => window.__tauRenameMock!.release());
  await expect(input).toBeEnabled();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue('Atlas Pending');
  expect(
    await input.evaluate((element) => {
      const field = element as HTMLInputElement;
      return field.selectionStart === field.selectionEnd;
    }),
  ).toBe(true);
});

test('deferred rename is optimistic, delays busy copy, and prevents duplicate submit or dismissal', async ({
  page,
}) => {
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await setMode(page, 'defer');
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Atlas Pending');
  await input.press('Enter');
  await expect(project(page).locator('.project-select')).toHaveText(
    'Atlas Pending',
  );
  await expect(
    dialog(page).getByRole('button', { name: 'Rename', exact: true }),
  ).toBeDisabled();
  await expect(
    dialog(page).getByRole('button', { name: 'Renaming…', exact: true }),
  ).toHaveCount(0);
  await expect(input).toBeDisabled();
  await expect(
    dialog(page).getByRole('button', { name: 'Cancel', exact: true }),
  ).toBeDisabled();
  await page.clock.runFor(300);
  await expect(
    dialog(page).getByRole('button', { name: 'Renaming…', exact: true }),
  ).toBeDisabled();
  await input.dispatchEvent('keydown', { key: 'Enter', bubbles: true });
  await dialog(page)
    .getByRole('button', { name: 'Renaming…', exact: true })
    .dispatchEvent('click');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Escape');
  await page.keyboard.press(await shortcut(page, 'k'));
  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expect(dialog(page)).toBeVisible();
  expect(await calls(page)).toEqual([
    { path: atlasPath, name: 'Atlas Pending' },
  ]);
  await page.evaluate(() => window.__tauRenameMock!.release());
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toBeFocused();
});

test('timeout permits closing, retains the draft, and fences a late result from another dialog', async ({
  page,
}) => {
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await setMode(page, 'defer');
  await openRename(page);
  const name = 'Atlas Waiting';
  await dialog(page).getByRole('textbox', { name: 'Project Name' }).fill(name);
  await dialog(page)
    .getByRole('button', { name: 'Rename', exact: true })
    .click();
  await page.clock.runFor(10_000);

  await expect(dialog(page).getByRole('alert')).toHaveText(
    'The rename has not been confirmed. You can close this dialog while saving finishes.',
  );
  await expect(
    dialog(page).getByRole('button', { name: 'Close', exact: true }),
  ).toBeEnabled();
  await dialog(page)
    .getByRole('button', { name: 'Close', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);

  await openRename(page, atlasPath, false);
  const retry = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await expect(retry).toHaveValue(name);
  await expect(retry).toBeDisabled();
  await retry.dispatchEvent('keydown', { key: 'Enter', bubbles: true });
  expect(await calls(page)).toEqual([{ path: atlasPath, name }]);
  await dialog(page)
    .getByRole('button', { name: 'Close', exact: true })
    .click();

  await openRename(page, notesPath);
  const newer = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await newer.fill('Notes draft');
  await page.evaluate(() => window.__tauRenameMock!.release());

  await expect(project(page).locator('.project-select')).toHaveText(name);
  await expect(dialog(page)).toBeVisible();
  await expect(newer).toHaveValue('Notes draft');
  expect(await calls(page)).toEqual([{ path: atlasPath, name }]);
  await page.keyboard.press('Escape');
});

test('Unicode names propagate to pickers and survive reorder and project switching with composer context intact', async ({
  page,
}) => {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const sessionId = await page
    .locator('.session-row.selected')
    .getAttribute('data-session-id');
  await composer.fill('Atlas composer stays here');
  await openRename(page);
  const name = 'Atlas 🪐 · 東京';
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill(`\u2003${name}\u00a0`);
  await page.keyboard.press(await shortcut(page, 'Enter'));
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toHaveText(name);

  await openPalette(page);
  await page.getByRole('option', { name: /Switch Session/ }).click();
  await expect(
    page.getByRole('dialog', { name: 'Switch Session' }),
  ).toContainText(name);
  await page.keyboard.press('Escape');
  await openPalette(page);
  await page
    .getByRole('option', { name: 'Move Project Down', exact: true })
    .click();
  await expect(page.locator('.project-group').last()).toHaveAttribute(
    'data-project-path',
    atlasPath,
  );
  await expect(page.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    sessionId!,
  );
  await expect(composer).toHaveValue('Atlas composer stays here');

  await openPalette(page);
  await page.getByRole('option', { name: /Switch Project/ }).click();
  await expect(
    page.getByRole('option', { name: new RegExp(name) }),
  ).toBeVisible();
  await page.getByRole('option', { name: /^notes/ }).click();
  await expect(composer).toBeEnabled();
  await composer.fill('Notes composer stays there');
  await openPalette(page);
  await page.getByRole('option', { name: /Switch Project/ }).click();
  await page.getByRole('option', { name: new RegExp(name) }).click();
  await expect(composer).toHaveValue('Atlas composer stays here');
  await expect(page.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    sessionId!,
  );
  expect(await calls(page)).toEqual([{ path: atlasPath, name }]);
});

test('late rejected save rolls back only its project and preserves another dialog draft', async ({
  page,
}) => {
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await setMode(page, 'defer-fail');
  await openRename(page);
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('Rejected Atlas');
  await page.keyboard.press('Enter');
  await expect.poll(() => calls(page)).toHaveLength(1);
  await expect(project(page).locator('.project-select')).toHaveText(
    'Rejected Atlas',
  );
  await page.clock.runFor(10_000);
  await dialog(page)
    .getByRole('button', { name: 'Close', exact: true })
    .click();
  await openRename(page, notesPath);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Unfinished Notes');
  await page.evaluate(() => window.__tauRenameMock!.release());
  await expect(project(page).locator('.project-select')).toHaveText('atlas');
  await expect(input).toHaveValue('Unfinished Notes');
  await expect(input).toBeFocused();
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await openRename(page);
  await expect(
    dialog(page).getByRole('textbox', { name: 'Project Name' }),
  ).toHaveValue('Rejected Atlas');
  await setMode(page, 'pass');
  await dialog(page)
    .getByRole('button', { name: 'Retry', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toHaveText(
    'Rejected Atlas',
  );
});

test('palette rename restores the original control on Cancel and successful save', async ({
  page,
}) => {
  const origin = project(page, notesPath).locator('.project-select');
  for (const save of [false, true]) {
    await origin.focus();
    await openPalette(page);
    await page.getByRole('option', { name: /Rename Project/ }).click();
    const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
    await expect(input).toBeFocused();
    await input.fill(save ? 'Palette Atlas' : 'Cancelled palette draft');
    if (save) await input.press('Enter');
    else
      await dialog(page)
        .getByRole('button', { name: 'Cancel', exact: true })
        .click();
    await expect(dialog(page)).toHaveCount(0);
    await expect(origin).toBeFocused();
    await expect(
      page.getByRole('dialog', { name: 'Command Palette' }),
    ).toHaveCount(0);
  }
  expect(await calls(page)).toEqual([
    { path: atlasPath, name: 'Palette Atlas' },
  ]);
});

test('rename blocks global shortcuts and Enter on Cancel does not submit', async ({
  page,
}) => {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Do not submit this draft');
  const sessionId = await page
    .locator('.session-row.selected')
    .getAttribute('data-session-id');
  const sessionCount = await page.locator('.session-row').count();
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Do not save this name');
  for (const key of ['k', 'p', 'Shift+p', 'Alt+o', 'f', 'Shift+a']) {
    await page.keyboard.press(await shortcut(page, key));
    await expect(dialog(page)).toBeVisible();
    await expect(input).toBeFocused();
    await expect(page.getByRole('dialog')).toHaveCount(1);
    await expect(page.getByRole('menu')).toHaveCount(0);
  }
  await expect(page.locator('.session-row')).toHaveCount(sessionCount);
  await expect(page.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    sessionId!,
  );
  await dialog(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .focus();
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-select')).toHaveText('atlas');
  await expect(composer).toHaveValue('Do not submit this draft');
  expect(await calls(page)).toEqual([]);
});

test('Escape closes the rename input context menu before the dialog', async ({
  page,
}) => {
  await openRename(page);
  const input = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await input.fill('Context menu draft');
  await input.click({ button: 'right' });
  await expect(
    page.getByRole('menuitem', { name: 'Cut', exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(dialog(page)).toBeVisible();
  await expect(input).toHaveValue('Context menu draft');
  await expect(input).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  expect(await calls(page)).toEqual([]);
});

for (const fails of [true, false]) {
  test(`late rename ${fails ? 'failure updates the live removal name' : 'success preserves newer removal focus'}`, async ({
    page,
  }) => {
    await page.clock.install();
    await page.clock.pauseAt(new Date());
    await setMode(page, fails ? 'defer-fail' : 'defer');
    await openRename(page);
    await dialog(page)
      .getByRole('textbox', { name: 'Project Name' })
      .fill('Pending Atlas');
    await page.keyboard.press('Enter');
    await expect.poll(() => calls(page)).toHaveLength(1);
    await expect(project(page).locator('.project-select')).toHaveText(
      'Pending Atlas',
    );
    await page.clock.runFor(10_000);
    await dialog(page)
      .getByRole('button', { name: 'Close', exact: true })
      .click();
    await openPalette(page);
    await page.getByRole('option', { name: /Remove Project/ }).click();
    const removal = page.locator('[role="alertdialog"].project-removal');
    const cancel = removal.getByRole('button', { name: 'Cancel', exact: true });
    await expect(removal).toContainText('“Pending Atlas”');
    await cancel.focus();
    await page.evaluate(() => window.__tauRenameMock!.release());
    await expect
      .poll(() => page.evaluate(() => window.__tauRenameMock!.settled))
      .toBe(1);
    const finalName = fails ? 'atlas' : 'Pending Atlas';
    await expect(project(page).locator('.project-select')).toHaveText(
      finalName,
    );
    // The rollback must update an already-open confirmation, not just its next opening.
    await expect(removal).toContainText(`“${finalName}”`);
    if (fails) await expect(removal).not.toContainText('Pending Atlas');
    await expect(cancel).toBeFocused();
    await expect(dialog(page)).toHaveCount(0);
    await cancel.click();
    await expect(project(page)).toBeVisible();
  });
}

test('remote project rename uses registry identity without starting an SSH connection', async ({
  page,
}) => {
  const selectedId = await page
    .locator('.session-row.selected')
    .getAttribute('data-session-id');
  await openRename(page, notesPath);
  await expect(dialog(page)).toContainText('ssh tau@buildbox');
  await expect(dialog(page)).toContainText('/srv/notes');
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('Remote Notes');
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page, notesPath).locator('.project-select')).toHaveText(
    'Remote Notes',
  );
  await expect(page.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    selectedId!,
  );
  expect(await calls(page)).toEqual([
    { path: notesPath, name: 'Remote Notes' },
  ]);
  await openRename(page, notesPath);
  await expect(
    dialog(page).getByRole('textbox', { name: 'Project Name' }),
  ).toHaveValue('Remote Notes');
  await expect(dialog(page)).toContainText('/srv/notes');
  expect(
    await page.evaluate(() => window.__tauRenameMock!.remoteCalls),
  ).toEqual([]);
});
