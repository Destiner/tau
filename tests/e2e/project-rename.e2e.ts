import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const root = '/browser-dev/projects';
const atlasPath = `${root}/atlas`;
const notesPath = `${root}/notes`;

type RenameMode = 'pass' | 'fail' | 'defer';

declare global {
  interface Window {
    __tauRenameMock?: {
      calls: { path: string; name: string }[];
      mode: RenameMode;
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
    .locator('.project-toggle')
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

async function installRenameStore(page: Page, archived = false): Promise<void> {
  // Persist only names across reloads; the sandbox remains the source of session/runtime data.
  await page.addInitScript((withArchive) => {
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
                if (mock.mode === 'fail')
                  throw new Error('Private registry failure');
                if (mock.mode === 'defer')
                  await new Promise<void>((resolve) => {
                    mock.release = resolve;
                  });
                const saved = await nextInvoke(command, args);
                const names = JSON.parse(
                  localStorage.getItem(storageKey) ?? '{}',
                ) as Record<string, string>;
                names[request.path] = saved as string;
                localStorage.setItem(storageKey, JSON.stringify(names));
                return saved;
              }
              const result = await nextInvoke(command, args);
              if (command !== 'load_workspace') return result;
              const workspace = structuredClone(result) as {
                projects: {
                  path: string;
                  name: string;
                  sessions: { archived: boolean; selected: boolean }[];
                }[];
              };
              const names = JSON.parse(
                localStorage.getItem(storageKey) ?? '{}',
              ) as Record<string, string>;
              for (const item of workspace.projects)
                item.name = names[item.path] ?? item.name;
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
  }, archived);
}

test.beforeEach(async ({ page }, testInfo) => {
  await installRenameStore(page, testInfo.title.startsWith('archived session'));
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
    project(page, notesPath).locator('.project-toggle'),
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
    await expect(project(page).locator('.project-toggle')).toBeFocused();
    await expect(composer).toHaveValue('Unsent composer draft');
    await expect(project(page).locator('.project-toggle')).toHaveText('atlas');
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
  await expect(project(page).locator('.project-toggle')).toHaveText(
    'Atlas Renamed',
  );
  expect(await calls(page)).toEqual([
    { path: atlasPath, name: 'Atlas Renamed' },
  ]);
});

test('unchanged Enter closes without a write', async ({ page }) => {
  await openRename(page);
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-toggle')).toBeFocused();
  expect(await calls(page)).toEqual([]);
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

test('accepts a duplicate project name without changing either project identity', async ({
  page,
}) => {
  const selected = page.locator('.session-row.selected');
  const sessionId = await selected.getAttribute('data-session-id');
  await openRename(page, notesPath);
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('atlas');
  await dialog(page)
    .getByRole('button', { name: 'Rename', exact: true })
    .click();

  await expect(project(page).locator('.project-toggle')).toHaveText('atlas');
  await expect(project(page, notesPath).locator('.project-toggle')).toHaveText(
    'atlas',
  );
  await expect(selected).toHaveAttribute('data-session-id', sessionId!);
  expect(await calls(page)).toEqual([{ path: notesPath, name: 'atlas' }]);
});

test('contains a 240-character name in the compact dialog at a narrow width', async ({
  page,
}) => {
  const name = 'a'.repeat(240);
  await page.setViewportSize({ width: 360, height: 720 });
  await openRename(page);
  const rename = dialog(page);
  const input = rename.getByRole('textbox', { name: 'Project Name' });
  await input.fill(name);

  await expect(input).toHaveAttribute('maxlength', '240');
  await expect(input).toHaveValue(name);
  await expect(rename).toHaveJSProperty(
    'scrollWidth',
    await rename.evaluate((element) => element.clientWidth),
  );
  const box = await rename.boundingBox();
  expect(box?.width).toBeLessThanOrEqual(324);
  await expect(input).toBeFocused();
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
  await expect(project(page).locator('.project-toggle')).toHaveText('atlas');
  await expect(input).toHaveValue('Atlas Retry');
  await expect(input).toBeEnabled();
  await expect(page.getByRole('alertdialog')).toHaveCount(0);
  await setMode(page, 'pass');
  await dialog(page)
    .getByRole('button', { name: 'Rename', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-toggle')).toHaveText(
    'Atlas Retry',
  );
  expect(await calls(page)).toEqual(
    Array.from({ length: 2 }, () => ({ path: atlasPath, name: 'Atlas Retry' })),
  );
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
  await expect(project(page).locator('.project-toggle')).toHaveText(
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
  await expect(project(page).locator('.project-toggle')).toBeFocused();
});

test('timeout permits cancellation, retains the draft, and fences a late result from another dialog', async ({
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
    dialog(page).getByRole('button', { name: 'Cancel', exact: true }),
  ).toBeEnabled();
  await dialog(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);

  await openRename(page, atlasPath, false);
  const retry = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await expect(retry).toHaveValue(name);
  await expect(retry).toBeDisabled();
  await retry.dispatchEvent('keydown', { key: 'Enter', bubbles: true });
  expect(await calls(page)).toEqual([{ path: atlasPath, name }]);
  await dialog(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();

  await openRename(page, notesPath);
  const newer = dialog(page).getByRole('textbox', { name: 'Project Name' });
  await newer.fill('Notes draft');
  await page.evaluate(() => window.__tauRenameMock!.release());

  await expect(project(page).locator('.project-toggle')).toHaveText(name);
  await expect(dialog(page)).toBeVisible();
  await expect(newer).toHaveValue('Notes draft');
  expect(await calls(page)).toEqual([{ path: atlasPath, name }]);
  await page.keyboard.press('Escape');
});

test('palette targets the active identity and refreshes sidebar, palette and removal labels', async ({
  page,
}) => {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Identity draft');
  const selected = page.locator('.session-row.selected');
  const sessionId = await selected.getAttribute('data-session-id');
  await project(page, notesPath).locator('.project-toggle').focus();
  await openPalette(page);
  const option = page.getByRole('option', { name: /Rename Project/ });
  await expect(option.locator('.command-palette-detail')).toHaveText(
    `atlas · ${atlasPath}`,
  );
  await option.click();
  await expect(
    dialog(page).getByRole('textbox', { name: 'Project Name' }),
  ).toHaveValue('atlas');
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('Atlas Display Name');
  await dialog(page)
    .getByRole('button', { name: 'Rename', exact: true })
    .click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(project(page).locator('.project-toggle')).toHaveText(
    'Atlas Display Name',
  );
  await expect(project(page, notesPath).locator('.project-toggle')).toHaveText(
    'notes',
  );
  await expect(selected).toHaveAttribute('data-session-id', sessionId!);
  await expect(composer).toHaveValue('Identity draft');
  await expect(
    project(page).getByRole('button', { name: 'Remove Atlas Display Name' }),
  ).toHaveCount(1);
  expect(await calls(page)).toEqual([
    { path: atlasPath, name: 'Atlas Display Name' },
  ]);
  await openPalette(page);
  for (const name of [/Rename Project/, /Remove Project/]) {
    await expect(
      page.getByRole('option', { name }).locator('.command-palette-detail'),
    ).toHaveText(`Atlas Display Name · ${atlasPath}`);
  }
  await page.getByRole('option', { name: /Remove Project/ }).click();
  const removal = page.locator('[role="alertdialog"].project-removal');
  await expect(removal).toContainText('“Atlas Display Name”');
  await removal.getByRole('button', { name: 'Cancel' }).click();
});

test('test-local IPC names survive reload without changing project or session identity', async ({
  page,
}) => {
  const sessionId = await page
    .locator('.session-row.selected')
    .getAttribute('data-session-id');
  await openRename(page);
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('Persistent Atlas');
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await expect(project(page).locator('.project-toggle')).toHaveText(
    'Persistent Atlas',
  );
  await expect(page.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    sessionId!,
  );
  await openRename(page);
  await expect(
    dialog(page).getByRole('textbox', { name: 'Project Name' }),
  ).toHaveValue('Persistent Atlas');
});

test('archived session rows use the renamed project label', async ({
  page,
}) => {
  await openRename(page);
  await dialog(page)
    .getByRole('textbox', { name: 'Project Name' })
    .fill('Archive Atlas');
  await page.keyboard.press('Enter');
  await expect(dialog(page)).toHaveCount(0);
  await page.getByRole('button', { name: 'Show Archived Sessions' }).click();
  await expect(page.locator('.archived-list .row .project')).toHaveText([
    'Archive Atlas',
  ]);
});
