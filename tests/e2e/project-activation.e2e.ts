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

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
});

test('project labels activate populated projects, expand them, and reuse the active session', async ({
  page,
}) => {
  const atlas = project(page, 'atlas');
  const notes = project(page, 'notes');
  const originalIds = await sessionIds(notes);
  expect(originalIds).toHaveLength(2);

  await notes.locator('.project-toggle').click();
  await expect(notes.locator('.session-list')).toHaveCount(0);
  await notes.locator('.project-select').click();
  await expect(notes.locator('.project-row')).toHaveClass(/selected/);
  await expect(notes.locator('.project-toggle')).toHaveAttribute(
    'aria-expanded',
    'true',
  );
  await expect(notes.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    'notes-inbox',
  );
  await expect(page.locator('.transcript')).toContainText(
    'Organize the notes inbox',
  );
  expect(await sessionIds(notes)).toEqual(originalIds);

  // Use a non-default session so activating the active project cannot silently
  // navigate back to its first session.
  await notes
    .locator('[data-session-id="notes-export"] .session-select')
    .click();
  await expect(page.locator('.transcript')).toContainText(
    'Sketch the Markdown export',
  );
  for (let i = 0; i < 3; i += 1) {
    await notes.locator('.project-select').click();
    await expect(notes.locator('.session-row.selected')).toHaveAttribute(
      'data-session-id',
      'notes-export',
    );
  }
  expect(await sessionIds(notes)).toEqual(originalIds);
  await expect(atlas.locator('.session-row')).toHaveCount(3);
});

test('empty project activation reveals exactly one session without leaving the sidebar and reuses it', async ({
  page,
}) => {
  await seedEmptyProject(page);
  await page.reload();
  const empty = project(page, 'empty');
  await expect(empty.locator('.project-select')).toBeVisible();
  await expect(empty.locator('.session-row')).toHaveCount(0);
  await empty.locator('.project-toggle').click();
  await expect(empty.locator('.project-toggle')).toHaveAttribute(
    'aria-expanded',
    'false',
  );
  await expect(empty.locator('.session-row')).toHaveCount(0);
  await expect(project(page, 'atlas').locator('.project-row')).toHaveClass(
    /selected/,
  );
  await empty.locator('.project-select').click();
  // Do not move the pointer: held sidebar ordering must not hide a new session.
  await expect
    .poll(() =>
      empty
        .locator('.project-select')
        .evaluate((element) => element.matches(':hover')),
    )
    .toBe(true);
  await expect(empty.locator('.session-row')).toHaveCount(1);
  await expect(empty.locator('.session-row.selected')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const ids = await sessionIds(empty);
  for (let i = 0; i < 3; i += 1) {
    await empty.locator('.project-select').click();
    await expect(empty.locator('.session-row')).toHaveCount(1);
    expect(await sessionIds(empty)).toEqual(ids);
  }
  // Untouched phantom sessions are intentionally discarded on navigation.
  await page
    .getByRole('textbox', { name: 'Message Pi' })
    .fill('Empty project draft');
  await project(page, 'atlas').locator('.project-select').click();
  await empty.locator('.project-select').click();
  await expect(empty.locator('.session-row.selected')).toHaveAttribute(
    'data-session-id',
    ids[0]!,
  );
  expect(await sessionIds(empty)).toEqual(ids);
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toHaveValue(
    'Empty project draft',
  );
});

test('drafts survive switching projects through their labels', async ({
  page,
}) => {
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Unsent Atlas draft');
  await project(page, 'notes').locator('.project-select').click();
  await expect(composer).toHaveValue('');
  await composer.fill('Unsent Notes draft');
  await project(page, 'atlas').locator('.project-select').click();
  await expect(composer).toHaveValue('Unsent Atlas draft');
  await project(page, 'notes').locator('.project-select').click();
  await expect(composer).toHaveValue('Unsent Notes draft');
});

test('chevron clicks and Left/Right on either header control only fold, while Enter/Space activate the label', async ({
  page,
}) => {
  const atlas = project(page, 'atlas');
  const notes = project(page, 'notes');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill('Keep Atlas active');
  const originalIds = await sessionIds(notes);
  const toggle = notes.locator('.project-toggle');
  const label = notes.locator('.project-select');

  async function expectAtlasActive(): Promise<void> {
    await expect(atlas.locator('.project-row')).toHaveClass(/selected/);
    await expect(notes.locator('.project-row')).not.toHaveClass(/selected/);
    await expect(composer).toHaveValue('Keep Atlas active');
  }

  for (const expanded of [false, true]) {
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', String(expanded));
    await expectAtlasActive();
  }
  for (const control of [label, toggle]) {
    await control.focus();
    for (const [key, expanded] of [
      ['ArrowLeft', false],
      ['ArrowRight', true],
    ] as const) {
      await control.press(key);
      await expect(toggle).toHaveAttribute('aria-expanded', String(expanded));
      await expect(control).toBeFocused();
      await expectAtlasActive();
    }
  }
  expect(await sessionIds(notes)).toEqual(originalIds);

  await label.focus();
  await page.keyboard.press('Tab');
  await expect(toggle).toBeFocused();
  for (const [key, expanded] of [
    ['Enter', false],
    ['Space', true],
  ] as const) {
    await toggle.press(key);
    await expect(toggle).toHaveAttribute('aria-expanded', String(expanded));
    await expectAtlasActive();
  }

  for (const key of ['Enter', 'Space']) {
    await atlas.locator('.project-select').click();
    await label.focus();
    await label.press('ArrowLeft');
    await label.press(key);
    await expect(notes.locator('.project-row')).toHaveClass(/selected/);
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(notes.locator('.session-row.selected')).toBeVisible();
    expect(await sessionIds(notes)).toEqual(originalIds);
  }
});

test('chevron is a fixed 24px sibling with transparent hover, independent of label hover and folding', async ({
  page,
}) => {
  const notes = project(page, 'notes');
  const label = notes.locator('.project-select');
  const toggle = notes.locator('.project-toggle');
  await expect(notes.locator('.project-select + .project-toggle')).toHaveCount(
    1,
  );
  await page.getByRole('textbox', { name: 'Message Pi' }).hover();
  const box = await toggle.boundingBox();
  if (!box) throw new Error('Expected a visible project chevron');
  expect(box.width).toBe(24);
  expect(box.height).toBe(24);

  for (const target of [label, toggle]) {
    await target.hover();
    await expect.poll(() => toggle.boundingBox()).toEqual(box);
    await expect
      .poll(() =>
        toggle.evaluate((element) => getComputedStyle(element).backgroundColor),
      )
      .toBe('rgba(0, 0, 0, 0)');
  }
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  expect(await toggle.boundingBox()).toEqual(box);
  await label.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  expect(await toggle.boundingBox()).toEqual(box);
});
