import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const root = '/browser-dev/projects';

function project(page: Page, name: string): Locator {
  return page.locator(`.project-group[data-project-path="${root}/${name}"]`);
}

async function sessionIds(group: Locator): Promise<(string | null)[]> {
  return group
    .locator('.session-row')
    .evaluateAll((rows) =>
      rows.map((row) => row.getAttribute('data-session-id')),
    );
}

// Import through the real sandbox API before the app reads its initial workspace.
async function seedEmptyProject(page: Page): Promise<void> {
  await page.addInitScript((path) => {
    type Internals = {
      invoke?: (command: string, args?: unknown) => Promise<unknown>;
    };
    let internals: Internals | undefined;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      get: (): Internals | undefined => internals,
      set: (next: Internals): void => {
        internals = next;
        let invoke: Internals['invoke'];
        Object.defineProperty(next, 'invoke', {
          configurable: true,
          get: (): Internals['invoke'] => invoke,
          set: (nextInvoke: NonNullable<Internals['invoke']>): void => {
            invoke = async (command, args): Promise<unknown> => {
              if (command === 'load_workspace') {
                await nextInvoke('import_project', { path });
              }
              return nextInvoke(command, args);
            };
          },
        });
      },
    });
  }, `${root}/empty`);
}

async function openWorkspace(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
}

async function expectPersistedFold(
  page: Page,
  name: string,
  collapsed: boolean,
): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(async (path) => {
        const workspace = (await window.__TAURI_INTERNALS__?.invoke(
          'load_workspace',
        )) as { projects: { path: string; collapsed: boolean }[] };
        return workspace.projects.find((item) => item.path === path)?.collapsed;
      }, `${root}/${name}`),
    )
    .toBe(collapsed);
}

test('both project disclosure controls fold without switching sessions or losing a draft', async ({
  page,
}) => {
  await openWorkspace(page);
  const atlas = project(page, 'atlas');
  const notes = project(page, 'notes');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });

  const selected = atlas.locator('[data-session-id="atlas-navigation"]');
  await selected.locator('.session-select').click();
  await expect(page.locator('.transcript')).toContainText(
    'Review the sidebar navigation states',
  );
  await composer.fill('Keep Atlas active');
  const originalNotesIds = await sessionIds(notes);

  for (const group of [notes, atlas]) {
    const name = group.locator('.project-select');
    const chevron = group.locator('.project-toggle');
    for (const control of [name, chevron]) {
      for (const expanded of [false, true]) {
        await control.click();
        await expect(name).toHaveAttribute('aria-expanded', String(expanded));
        await expect(chevron).toHaveAttribute(
          'aria-expanded',
          String(expanded),
        );
        await expect(group.locator('.session-list')).toHaveCount(
          expanded ? 1 : 0,
        );
        await expect(atlas.locator('.project-row')).toHaveClass(/selected/);
        await expect(page.locator('.transcript')).toContainText(
          'Review the sidebar navigation states',
        );
        await expect(composer).toHaveValue('Keep Atlas active');
      }
    }
  }
  await expect(selected.locator('.session-select')).toHaveAttribute(
    'aria-current',
    'page',
  );
  expect(await sessionIds(notes)).toEqual(originalNotesIds);

  await notes.locator('.project-select').click();
  await expectPersistedFold(page, 'notes', true);
  await notes.locator('.project-toggle').click();
  await expectPersistedFold(page, 'notes', false);
});

test('the project plus creates a fresh selected session without moving the pointer', async ({
  page,
}) => {
  await openWorkspace(page);
  const atlas = project(page, 'atlas');
  const notes = project(page, 'notes');
  const originalIds = await sessionIds(notes);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Original Atlas draft');

  const plus = notes.getByRole('button', { name: 'New Session in notes' });
  await plus.click();
  await expect(notes.locator('.session-row')).toHaveCount(
    originalIds.length + 1,
  );
  const selected = notes.locator('.session-row.selected');
  await expect(selected).toBeVisible();
  const id = await selected.getAttribute('data-session-id');
  expect(id).toBeTruthy();
  expect(originalIds).not.toContain(id);
  expect(await plus.evaluate((element) => element.matches(':hover'))).toBe(
    true,
  );
  await expect(composer).toBeEnabled();
  await expect(composer).toHaveValue('');

  await atlas
    .locator('[data-session-id="atlas-overview"] .session-select')
    .click();
  await expect(composer).toHaveValue('Original Atlas draft');
});

test('disclosing an empty project does not create a session, but its plus opens one', async ({
  page,
}) => {
  await seedEmptyProject(page);
  await openWorkspace(page);
  const empty = project(page, 'empty');
  const name = empty.locator('.project-select');
  const chevron = empty.locator('.project-toggle');
  const plus = empty.getByRole('button', { name: 'New Session in empty' });

  await name.click();
  await expect(empty.getByText('No active sessions')).toHaveCount(0);
  await chevron.click();
  await expect(empty.getByText('No active sessions')).toBeVisible();
  await expect(empty.locator('.session-row')).toHaveCount(0);
  await expect(project(page, 'atlas').locator('.project-row')).toHaveClass(
    /selected/,
  );
  await name.click();
  await expectPersistedFold(page, 'empty', true);

  await plus.click();
  await expect(name).toHaveAttribute('aria-expanded', 'true');
  await expect(empty.locator('.session-row')).toHaveCount(1);
  await expect(empty.locator('.session-row.selected')).toBeVisible();
  expect(await plus.evaluate((element) => element.matches(':hover'))).toBe(
    true,
  );
  await expectPersistedFold(page, 'empty', false);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
});

test('project actions reveal for hover and keyboard focus, not lingering pointer focus', async ({
  page,
}) => {
  await openWorkspace(page);
  const notes = project(page, 'notes');
  const name = notes.locator('.project-select');
  const chevron = notes.locator('.project-toggle');
  const plus = notes.getByRole('button', { name: 'New Session in notes' });
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Keep this draft');

  await name.click();
  await expect(name).toHaveAttribute('aria-expanded', 'false');
  await expect(chevron).toHaveCSS('opacity', '0.65');
  await expect(plus).toHaveCSS('opacity', '0.65');
  await composer.hover();
  await expect(chevron).toHaveCSS('opacity', '0');
  await expect(plus).toHaveCSS('opacity', '0');

  // Focus the anchor explicitly: mouse clicks do not focus buttons on WebKit.
  await name.focus();
  await page.keyboard.press('Tab');
  await expect(chevron).toBeFocused();
  await expect(chevron).toHaveCSS('opacity', '1');
  await page.keyboard.press('Tab');
  await expect(plus).toBeFocused();
  await expect(plus).toHaveCSS('opacity', '1');
  await page.keyboard.press('Shift+Tab');
  await expect(chevron).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(name).toBeFocused();

  for (const control of [name, chevron]) {
    await control.focus();
    for (const [key, expanded] of [
      ['ArrowRight', true],
      ['ArrowLeft', false],
      ['Enter', true],
      ['Space', false],
    ] as const) {
      await page.keyboard.press(key);
      await expect(name).toHaveAttribute('aria-expanded', String(expanded));
      await expect(control).toBeFocused();
      await expect(composer).toHaveValue('Keep this draft');
    }
  }

  await page.keyboard.press('Tab');
  await expect(plus).toBeFocused();
  await page.keyboard.press('Space');
  await expect(notes.locator('.session-row')).toHaveCount(3);
  await expect(notes.locator('.session-row.selected')).toBeVisible();
});
