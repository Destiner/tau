import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const root = '/browser-dev/projects';
const atlasPath = `${root}/atlas`;
const notesPath = `${root}/notes`;

type RemovalMockMode = 'pass' | 'defer' | 'fail';

declare global {
  interface Window {
    __tauRemovalMock?: {
      calls: number;
      mode: RemovalMockMode;
      release: () => void;
    };
  }
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

function project(path: string): string {
  return `.project-group[data-project-path="${path}"]`;
}

async function installRemovalMock(
  page: Page,
  mode: RemovalMockMode = 'pass',
): Promise<void> {
  await page.evaluate((nextMode): void => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const invoke = internals.invoke.bind(internals);
    let release: (() => void) | undefined;
    window.__tauRemovalMock = {
      calls: 0,
      mode: nextMode,
      release: (): void => {
        release?.();
      },
    };
    internals.invoke = async (command, args): Promise<unknown> => {
      if (command !== 'remove_project') return invoke(command, args);
      const mock = window.__tauRemovalMock!;
      mock.calls += 1;
      if (mock.mode === 'fail') throw new Error('Registry unavailable');
      if (mock.mode === 'defer') {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return null;
    };
  }, mode);
}

async function expectValidFocus(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const control = document.activeElement;
        return (
          control instanceof HTMLElement &&
          control.matches(
            'button:not(:disabled), input:not(:disabled), textarea:not(:disabled)',
          )
        );
      }),
    )
    .toBe(true);
}

async function removalCalls(page: Page): Promise<number> {
  return page.evaluate(() => window.__tauRemovalMock?.calls ?? 0);
}

async function interceptInitialProjectName(
  page: Page,
  name: string,
): Promise<void> {
  await page.addInitScript((projectName) => {
    type TauriInternals = {
      invoke?: (command: string, args?: unknown) => Promise<unknown>;
    };
    const tauWindow = window as typeof window & {
      __TAURI_INTERNALS__?: TauriInternals;
    };
    let internals: TauriInternals | undefined;

    Object.defineProperty(tauWindow, '__TAURI_INTERNALS__', {
      configurable: true,
      get: (): TauriInternals | undefined => internals,
      set: (next: TauriInternals): void => {
        internals = next;
        let invoke: TauriInternals['invoke'];
        Object.defineProperty(next, 'invoke', {
          configurable: true,
          get: (): TauriInternals['invoke'] => invoke,
          set: (nextInvoke: NonNullable<TauriInternals['invoke']>): void => {
            invoke = async (command, args): Promise<unknown> => {
              const result = await nextInvoke(command, args);
              if (command !== 'load_workspace') return result;
              const workspace = structuredClone(result) as {
                projects: { name: string }[];
              };
              workspace.projects[0]!.name = projectName;
              return workspace;
            };
          },
        });
      },
    });
  }, name);
}

async function openSidebarRemoval(page: Page, path = atlasPath): Promise<void> {
  await page.locator(project(path)).hover();
  await page
    .locator(project(path))
    .locator('.project-toggle')
    .click({ button: 'right' });
  await page
    .getByRole('menuitem', { name: 'Remove Project', exact: true })
    .click();
}

async function expectRemovalDialog(page: Page, name: string): Promise<void> {
  const dialog = page.locator('[role="alertdialog"].project-removal');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading')).toHaveText('Remove project?');
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await expect(dialog).toContainText(`“${name}”`);
  await expect(dialog).not.toContainText(root);
  await expect(dialog).not.toContainText('Project files and');
}

test('sidebar and palette request removal without mutating until confirmation', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page);

  await openSidebarRemoval(page, notesPath);
  await expectRemovalDialog(page, 'notes');
  await expect(page.locator(project(notesPath))).toBeVisible();
  expect(await removalCalls(page)).toBe(0);
  await page.getByRole('button', { name: 'Cancel' }).click();

  await page.locator(project(notesPath)).locator('.project-toggle').focus();
  await openPalette(page);
  const remove = page.getByRole('option', { name: /Remove Project/ });
  await expect(remove.locator('.command-palette-detail')).toHaveText(
    'atlas · /browser-dev/projects/atlas',
  );
  await remove.click();
  await expectRemovalDialog(page, 'atlas');
  expect(await removalCalls(page)).toBe(0);
});

test('Cancel, Escape, and backdrop preserve the draft and do not invoke removal', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.fill('Draft that must survive a cancelled removal');
  await installRemovalMock(page);

  for (const dismiss of ['button', 'escape', 'backdrop'] as const) {
    await openSidebarRemoval(page);
    await expectRemovalDialog(page, 'atlas');
    if (dismiss === 'button')
      await page.getByRole('button', { name: 'Cancel' }).click();
    else if (dismiss === 'escape') await page.keyboard.press('Escape');
    else
      await page
        .locator('.project-removal-overlay')
        .click({ position: { x: 4, y: 4 } });

    await expect(page.locator('.project-removal')).toHaveCount(0);
    await expect(composer).toHaveValue(
      'Draft that must survive a cancelled removal',
    );
    await expect(page.locator(project(atlasPath))).toBeVisible();
    await expectValidFocus(page);
  }
  expect(await removalCalls(page)).toBe(0);
});

test('Cancel focused by default also cancels on Enter', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page);
  await openSidebarRemoval(page);
  await expectRemovalDialog(page, 'atlas');

  await page.keyboard.press('Enter');

  await expect(page.locator('.project-removal')).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toBeVisible();
  await expectValidFocus(page);
  expect(await removalCalls(page)).toBe(0);
});

test('project removal traps Tab and Shift+Tab between its actions', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openSidebarRemoval(page);

  const dialog = page.locator('[role="alertdialog"].project-removal');
  const cancel = dialog.getByRole('button', { name: 'Cancel' });
  const remove = dialog.getByRole('button', { name: 'Remove' });
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(remove).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(remove).toBeFocused();
});

test('composition Mod+Enter does not confirm, but a real shortcut does', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page);
  await openSidebarRemoval(page);

  const dialog = page.locator('[role="alertdialog"].project-removal');
  const composingPrevented = await dialog.evaluate((element) => {
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      metaKey: true,
      ctrlKey: true,
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    element.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(composingPrevented).toBe(false);
  await expect(dialog).toBeVisible();
  expect(await removalCalls(page)).toBe(0);

  await page.keyboard.press(await shortcut(page, 'Enter'));
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toHaveCount(0);
  expect(await removalCalls(page)).toBe(1);
});

test('project removal blocks reopening the command palette', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openSidebarRemoval(page);
  await expectRemovalDialog(page, 'atlas');

  await page.keyboard.press(await shortcut(page, 'k'));

  await expect(
    page.getByRole('dialog', { name: 'Command Palette' }),
  ).toHaveCount(0);
  await expectRemovalDialog(page, 'atlas');
});

test('confirmation removes inactive, active, and final projects', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page);

  await openSidebarRemoval(page, notesPath);
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(page.locator(project(notesPath))).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toBeVisible();

  await openSidebarRemoval(page, atlasPath);
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(page.locator(project(atlasPath))).toHaveCount(0);
  await expect(
    page.getByRole('button', { name: 'Open Local Project' }),
  ).toBeFocused();
  expect(await removalCalls(page)).toBe(2);
});

test('removing the active project clears selection and restores valid focus', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await installRemovalMock(page);
  await openSidebarRemoval(page, atlasPath);
  await page.getByRole('button', { name: 'Remove' }).click();

  await expect(page.locator(project(atlasPath))).toHaveCount(0);
  await expect(page.locator(project(notesPath))).toBeVisible();
  await expect(page.locator('.session-row.selected')).toHaveCount(0);
  await expectValidFocus(page);
  expect(await removalCalls(page)).toBe(1);
});

test('a narrow removal confirmation wraps a long project name from IPC', async ({
  page,
}) => {
  const longName =
    'a deliberately long project name that remains readable in a narrow confirmation';
  await interceptInitialProjectName(page, longName);
  await page.setViewportSize({ width: 480, height: 720 });
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openSidebarRemoval(page);

  const dialog = page.locator('[role="alertdialog"].project-removal');
  const description = dialog.locator('.project-removal-description');
  await expect(description).toContainText(`“${longName}”`);
  await expect(description).toHaveJSProperty(
    'scrollWidth',
    await description.evaluate((element) => element.clientWidth),
  );
  const box = await dialog.boundingBox();
  expect(box?.width).toBeLessThanOrEqual(444);
});

test('a pending confirmation prevents duplicate submission and unsafe dismissal', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page, 'defer');
  await openSidebarRemoval(page);

  const dialog = page.locator('[role="alertdialog"].project-removal');
  const remove = dialog.getByRole('button', { name: 'Remove' });
  await remove.click();
  await expect(dialog).toHaveAttribute('aria-busy', 'true');
  await expect(remove).toBeDisabled();
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeDisabled();
  await page.keyboard.press('Escape');
  await page
    .locator('.project-removal-overlay')
    .click({ position: { x: 4, y: 4 } });
  await expect(dialog).toBeVisible();
  expect(await removalCalls(page)).toBe(1);

  await page.evaluate(() => {
    window.__tauRemovalMock?.release();
  });
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toHaveCount(0);
  expect(await removalCalls(page)).toBe(1);
});

test('a persistence failure stays inline and Remove retries it', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await installRemovalMock(page, 'fail');
  await openSidebarRemoval(page);

  const dialog = page.locator('[role="alertdialog"].project-removal');
  await dialog.getByRole('button', { name: 'Remove' }).click();
  await expect(dialog.getByRole('alert')).toHaveText(
    'The project could not be removed. Try again.',
  );
  await expect(page.getByRole('dialog', { name: /could not/i })).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toBeVisible();
  expect(await removalCalls(page)).toBe(1);

  await page.evaluate(() => {
    if (window.__tauRemovalMock) window.__tauRemovalMock.mode = 'pass';
  });
  await dialog.getByRole('button', { name: 'Remove' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(project(atlasPath))).toHaveCount(0);
  expect(await removalCalls(page)).toBe(2);
});

test('SSH removal warns that Tau forgets its saved-session list', async ({
  page,
}) => {
  await page.goto('/?test-project-removal=ssh');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openSidebarRemoval(page);
  const dialog = page.locator('[role="alertdialog"].project-removal');
  await expect(dialog).toContainText('“atlas” on buildbox');
  await expect(dialog).toContainText(
    'Its saved-session list will also be removed from Tau.',
  );
});

test('Escape dismisses only a native quit confirmation above project removal', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  await openSidebarRemoval(page);
  await expectRemovalDialog(page, 'atlas');

  await page.evaluate(async () => {
    const internals = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const invoke = internals.invoke.bind(internals);
    internals.invoke = async (command, args): Promise<unknown> => {
      if (command === 'resolve_quit_request') {
        return !(args as { confirmed: boolean }).confirmed;
      }
      return invoke(command, args);
    };
    await internals.invoke('plugin:event|emit', {
      event: 'tau://quit-requested',
      payload: { requestId: 99, intent: 'ordinary' },
    });
  });
  const quit = page.locator('[role="alertdialog"].quit-confirmation');
  await expect(quit).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(quit).toHaveCount(0);
  await expect(
    page.locator('[role="alertdialog"].project-removal'),
  ).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(
    page.locator('[role="alertdialog"].project-removal'),
  ).toHaveCount(0);
  await expectValidFocus(page);
});
