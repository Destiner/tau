import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const searchInput = (page: Page): Locator =>
  page.getByRole('textbox', { name: 'Find in Transcript', exact: true });
const searchBar = (page: Page): Locator =>
  page.getByRole('search', { name: 'Find in Transcript', includeHidden: true });

async function shortcut(page: Page, key: string): Promise<void> {
  const mac = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform),
  );
  await page.keyboard.press(`${mac ? 'Meta' : 'Control'}+${key}`);
}

async function query(page: Page, text: string, status: string): Promise<void> {
  await searchInput(page).fill(text);
  await expect(searchBar(page).locator('[aria-live="polite"]')).toHaveText(
    status,
  );
}

// Assert the actual painted range, not just that its virtual row is mounted.
async function expectActiveRange(
  page: Page,
  text: string,
  id?: string,
): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const ranges = CSS.highlights?.get('tau-transcript-active');
        if (!ranges || ranges.size !== 1) return null;
        const range = [...ranges][0] as Range;
        const rect = range.getBoundingClientRect();
        const viewport = document
          .querySelector('[aria-label="Transcript"]')
          ?.getBoundingClientRect();
        const parent = range.startContainer.parentElement;
        return {
          text: range.toString(),
          id: parent
            ?.closest('[data-message-id]')
            ?.getAttribute('data-message-id'),
          visible: Boolean(
            viewport &&
            rect.width > 0 &&
            rect.height > 0 &&
            rect.top >= viewport.top &&
            rect.bottom <= viewport.bottom &&
            rect.left >= viewport.left &&
            rect.right <= viewport.right,
          ),
        };
      }),
    )
    .toMatchObject({ text, visible: true, ...(id ? { id } : {}) });
}

test('App shortcut and palette preserve the draft, selection, query and navigation', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Draft-only-needle stays untouched');
  await composer.evaluate((element: HTMLTextAreaElement) =>
    element.setSelectionRange(4, 13, 'backward'),
  );
  await shortcut(page, 'f');
  await expect(searchInput(page)).toBeFocused();
  await query(page, 'Atlas', '1 / 2');
  await expectActiveRange(page, 'Atlas');
  await searchInput(page).press('Enter');
  await expect(searchBar(page)).toContainText('2 / 2');
  await searchInput(page).press('Enter');
  await expect(searchBar(page)).toContainText('1 / 2');
  await searchInput(page).press('Shift+Enter');
  await expect(searchBar(page)).toContainText('2 / 2');
  await page
    .getByRole('button', { name: 'Previous Match', exact: true })
    .click();
  await expect(searchBar(page)).toContainText('1 / 2');
  await page.getByRole('button', { name: 'Next Match', exact: true }).click();
  await expect(searchBar(page)).toContainText('2 / 2');
  await shortcut(page, 'f');
  await expect(searchInput(page)).toBeFocused();
  await expect
    .poll(() =>
      searchInput(page).evaluate((input: HTMLInputElement) => [
        input.selectionStart,
        input.selectionEnd,
      ]),
    )
    .toEqual([0, 5]);
  await searchInput(page).press('Escape');
  await expect(searchBar(page)).toHaveCount(0);
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('Draft-only-needle stays untouched');
  await expect
    .poll(() =>
      composer.evaluate((input: HTMLTextAreaElement) => [
        input.selectionStart,
        input.selectionEnd,
        input.selectionDirection,
      ]),
    )
    .toEqual([4, 13, 'backward']);

  await shortcut(page, 'k');
  const palette = page.getByRole('dialog', {
    name: 'Command Palette',
    exact: true,
  });
  await palette.getByRole('combobox').fill('Find in Transcript');
  await palette.getByRole('option', { name: /Find in Transcript/ }).click();
  await expect(palette).toHaveCount(0);
  await expect(searchInput(page)).toBeFocused();
  await expect(searchInput(page)).toHaveValue('Atlas');
  await expect(searchBar(page)).toContainText('2 / 2');
  await query(page, 'Draft-only-needle', 'No matches');
  await expect(
    page.getByRole('button', { name: 'Next Match', exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole('button', { name: 'Previous Match', exact: true }),
  ).toBeDisabled();
  await searchInput(page).press('Enter');
  await expect(searchInput(page)).toBeFocused();
  await page.getByRole('button', { name: 'Close Find', exact: true }).click();
  await expect(composer).toBeFocused();
  await shortcut(page, 'f');
  await expect(searchInput(page)).toHaveValue('Draft-only-needle');
  await query(page, '', '');
  await expect
    .poll(() =>
      page.evaluate(
        () => CSS.highlights?.get('tau-transcript-active')?.size ?? 0,
      ),
    )
    .toBe(0);
});

test('Escape from the composer closes Find without changing the draft', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Keep this ordinary draft');
  await shortcut(page, 'f');
  await query(page, 'Atlas', '1 / 2');
  await composer.focus();
  await composer.press('Escape');
  await expect(searchBar(page)).toHaveCount(0);
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('Keep this ordinary draft');
});

test('Escape dismisses the slash menu before Find when the composer is focused', async ({
  page,
}) => {
  // The ordinary sandbox has transcript content but no extension commands.
  await page.addInitScript(() => {
    let internals: typeof window.__TAURI_INTERNALS__;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      get: () => internals,
      set: (next) => {
        internals = next;
        if (!internals) return;
        let transform = internals.transformCallback;
        Object.defineProperty(internals, 'transformCallback', {
          configurable: true,
          get: () => transform,
          set: (register: typeof transform) => {
            transform = (callback: unknown): unknown =>
              register((value: unknown): unknown => {
                const event = value as {
                  payload?: { kind?: string; line?: string };
                };
                if (event.payload?.kind === 'rpc' && event.payload.line) {
                  const response = JSON.parse(event.payload.line);
                  if (
                    response.type === 'response' &&
                    response.command === 'get_commands'
                  ) {
                    response.data = {
                      commands: [
                        {
                          name: 'regression',
                          description: 'Regression command',
                          source: 'extension',
                        },
                      ],
                    };
                    event.payload.line = JSON.stringify(response);
                  }
                }
                return typeof callback === 'function'
                  ? callback(value)
                  : undefined;
              });
          },
        });
      },
    });
  });
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await shortcut(page, 'f');
  await expect(searchInput(page)).toBeFocused();
  await composer.fill('/');
  await expect(page.locator('.command-menu')).toBeVisible();
  await composer.press('Escape');
  await expect(page.locator('.command-menu')).toHaveCount(0);
  await expect(searchBar(page)).toBeVisible();
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('/');
  await composer.press('Escape');
  await expect(searchBar(page)).toHaveCount(0);
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('/');
});

for (const input of ['wheel', 'touchstart', 'pointerdown', 'PageUp'] as const) {
  test(`${input} cancels an in-flight reveal without overriding the reader offset`, async ({
    page,
  }) => {
    await page.goto('/?fixture=long-transcript&search=true');
    await query(page, 'Search checkpoint', '1 / 3');
    await expectActiveRange(page, 'Search checkpoint', 'search-early');
    const result = await page.evaluate(async (input) => {
      const transcript = document.querySelector<HTMLElement>(
        '[aria-label="Transcript"]',
      )!;
      const find = document.querySelector<HTMLInputElement>(
        'input[aria-label="Find in Transcript"]',
      )!;
      const frame = (): Promise<void> =>
        new Promise((resolve) => requestAnimationFrame(() => resolve()));
      // The reveal loop is bounded to 16 frames; let the first match finish.
      for (let i = 0; i < 24; i++) await frame();
      const original = transcript.scrollTo;
      let interrupted = false;
      const offsets: number[] = [];
      transcript.scrollTo = function (
        options?: ScrollToOptions | number,
        y?: number,
      ): void {
        const scroll = original.bind(this);
        if (typeof options === 'number') scroll(options, y ?? 0);
        else scroll(options);
        if (interrupted) return;
        interrupted = true;
        // Interrupt synchronously inside the first virtualizer scroll, before
        // the reveal can resume on its next animation frame.
        const event =
          input === 'wheel'
            ? new WheelEvent('wheel', { bubbles: true, deltaY: -400 })
            : input === 'pointerdown'
              ? new PointerEvent('pointerdown', {
                  bubbles: true,
                  clientX: transcript.getBoundingClientRect().right - 1,
                })
              : input === 'PageUp'
                ? new KeyboardEvent('keydown', { bubbles: true, key: input })
                : new Event('touchstart', { bubbles: true });
        transcript.dispatchEvent(event);
        // Model the native scroll following wheel/scrollbar/key input. The
        // synthetic event deliberately has no browser scrolling default.
        transcript.scrollTop = 0;
      };
      try {
        find.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }),
        );
        for (let i = 0; i < 24; i++) {
          await frame();
          offsets.push(transcript.scrollTop);
        }
      } finally {
        transcript.scrollTo = original;
      }
      return { interrupted, offsets };
    }, input);
    expect(result.interrupted).toBe(true);
    expect(result.offsets).toHaveLength(24);
    expect(Math.max(...result.offsets.map(Math.abs))).toBeLessThan(2);
    await expect(searchBar(page)).toContainText('2 / 3');
  });
}

test('find reveals early, middle and late matches across 5000 virtual rows', async ({
  page,
}) => {
  await page.goto('/?fixture=long-transcript&search=true');
  await expect(page.getByTestId('fixture-count')).toHaveText('5000 messages');
  await expect(page.locator('[data-index="4999"]')).toBeVisible();
  await query(page, 'Search checkpoint', '1 / 3');
  for (const [index, label] of ['early', 'middle', 'late'].entries()) {
    await expect(searchBar(page)).toContainText(`${index + 1} / 3`);
    await expectActiveRange(page, 'Search checkpoint', `search-${label}`);
    expect(await page.locator('[data-message-id]').count()).toBeLessThan(50);
    await searchInput(page).press('Enter');
  }
  await expect(searchBar(page)).toContainText('1 / 3');
  await expectActiveRange(page, 'Search checkpoint', 'search-early');
  await searchInput(page).press('Shift+Enter');
  await expectActiveRange(page, 'Search checkpoint', 'search-late');
});

test('highlights across formatting and syntax tokens, excluding activity and controls', async ({
  page,
}) => {
  await page.goto(
    '/?fixture=long-transcript&search=true&count=12&compacted=true',
  );
  await expect(searchBar(page)).not.toContainText('Loaded messages only');
  await expect(searchInput(page)).toHaveAttribute('placeholder', 'Search');
  await query(page, 'Cross format needle', '1 / 3');
  await expectActiveRange(page, 'Cross format needle');
  await query(page, 'const codeNeedle', '1 / 3');
  await expectActiveRange(page, 'const codeNeedle');
  for (const text of [
    'excluded-thinking-needle',
    'excluded-tool-needle',
    'excluded-argument-needle',
    'excluded-result-needle',
    'excluded-notice-needle',
    'Copy Code',
    'Load Earlier Messages',
  ]) {
    await query(page, text, 'No matches');
    await expect
      .poll(() =>
        page.evaluate(
          () => CSS.highlights?.get('tau-transcript-matches')?.size ?? 0,
        ),
      )
      .toBe(0);
  }
});

test('streaming updates search without moving a reader away from a historical match', async ({
  page,
}) => {
  await page.goto('/?fixture=long-transcript&search=true');
  await query(page, 'Search checkpoint', '1 / 3');
  await searchInput(page).press('Enter');
  await expectActiveRange(page, 'Search checkpoint', 'search-middle');
  const transcript = page.getByLabel('Transcript', { exact: true });
  const before = await transcript.evaluate((element) => element.scrollTop);
  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );
  await expect(searchBar(page)).toContainText('2 / 3');
  await expectActiveRange(page, 'Search checkpoint', 'search-middle');
  await expect
    .poll(() =>
      transcript.evaluate(
        (element, top) => Math.abs(element.scrollTop - top),
        before,
      ),
    )
    .toBeLessThan(2);
  await query(page, 'Streaming fixture chunk', '1 / 18');
  await expectActiveRange(page, 'Streaming fixture chunk', 'search-late');
});

test('App sessions keep independent find queries, visibility, selection and drafts', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Overview draft');
  await shortcut(page, 'f');
  await query(page, 'Atlas', '1 / 2');
  await searchInput(page).press('Enter');
  await page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .getByRole('button', { name: /^Navigation review / })
    .click();
  await expect(searchBar(page)).toHaveCount(0);
  await expect(composer).toHaveValue('');
  await composer.fill('Navigation draft');
  await shortcut(page, 'f');
  await query(page, 'sidebar', '1 / 1');
  await page.getByRole('button', { name: 'Close Find', exact: true }).click();
  await page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .getByRole('button', { name: /^Draft Workspace overview / })
    .click();
  await expect(searchInput(page)).toHaveValue('Atlas');
  await expect(searchBar(page)).toContainText('2 / 2');
  await expect(composer).toHaveValue('Overview draft');
  await page
    .getByRole('complementary', { name: 'Projects and Sessions' })
    .getByRole('button', { name: /^Draft Navigation review / })
    .click();
  await expect(searchBar(page)).toHaveCount(0);
  await expect(composer).toHaveValue('Navigation draft');
  await shortcut(page, 'f');
  await expect(searchInput(page)).toHaveValue('sidebar');
  await expect(searchBar(page)).toContainText('1 / 1');
});

test('find indexes hydrated history in a deterministic full-app Pi scenario', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-history');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Keep this unsent scenario draft');
  await shortcut(page, 'f');
  await query(page, 'History prompt', '1 / 12');
  await expectActiveRange(page, 'History prompt');
  await searchInput(page).press('Shift+Enter');
  await expect(searchBar(page)).toContainText('12 / 12');
  await expectActiveRange(page, 'History prompt');
  await searchInput(page).press('Escape');
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue('Keep this unsent scenario draft');
});

test('Escape from a palette above Find restores the search caret without closing Find', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Draft beneath both overlays');
  await shortcut(page, 'f');
  await query(page, 'Atlas', '1 / 2');
  await searchInput(page).evaluate((input: HTMLInputElement) =>
    input.setSelectionRange(2, 2),
  );
  await shortcut(page, 'k');
  const palette = page.getByRole('dialog', {
    name: 'Command Palette',
    exact: true,
  });
  await expect(palette).toBeVisible();
  await palette.getByRole('combobox').press('Escape');
  await expect(palette).toHaveCount(0);
  await expect(searchInput(page)).toBeFocused();
  await expect(searchInput(page)).toHaveValue('Atlas');
  await expect(searchBar(page)).toContainText('1 / 2');
  await expect
    .poll(() =>
      searchInput(page).evaluate((input: HTMLInputElement) => [
        input.selectionStart,
        input.selectionEnd,
      ]),
    )
    .toEqual([2, 2]);
  await expect(composer).toHaveValue('Draft beneath both overlays');
});

test('Find is unavailable during a dirty session rename without losing either draft', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-bootstrap');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Unsent composer draft');
  await page.getByRole('button', { name: 'Main', exact: true }).click();
  const name = page.getByRole('textbox', { name: 'Session Name', exact: true });
  await name.fill('Uncommitted search rename');
  await shortcut(page, 'f');
  await expect(name).toBeFocused();
  await expect(name).toHaveValue('Uncommitted search rename');
  await expect(searchBar(page)).toHaveCount(0);
  await shortcut(page, 'k');
  const palette = page.getByRole('dialog', {
    name: 'Command Palette',
    exact: true,
  });
  await expect(palette).toBeVisible();
  await palette.getByRole('combobox').fill('Find in Transcript');
  await expect(
    palette.getByRole('option', { name: /Find in Transcript/ }),
  ).toHaveCount(0);
  await palette.getByRole('combobox').press('Escape');
  await expect(name).toBeFocused();
  await expect(name).toHaveValue('Uncommitted search rename');
  await expect(composer).toHaveValue('Unsent composer draft');
});

test('Find is unavailable in a fullscreen viewer without losing the composer draft', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill(
    'Render this diagram:\n\n```mermaid\ngraph LR\nA[Start] --> B[Finish]\n```\n',
  );
  await composer.press('Enter');
  const expand = page.locator('.diagram-expand').last();
  await expect(expand).toBeAttached();
  await composer.fill('Viewer must preserve this draft');
  await expand.locator('..').hover();
  await expand.click();
  const viewer = page.getByRole('dialog', { name: 'Diagram', exact: true });
  await expect(viewer).toBeVisible();
  await shortcut(page, 'f');
  await expect(searchBar(page)).toHaveCount(0);
  await expect(viewer).toBeVisible();
  await expect(
    page.getByRole('textbox', {
      name: 'Message Pi',
      exact: true,
      includeHidden: true,
    }),
  ).toHaveValue('Viewer must preserve this draft');
  await page.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await expect(composer).toHaveValue('Viewer must preserve this draft');
  await shortcut(page, 'f');
  await expect(searchInput(page)).toBeFocused();
});

test.describe('inline Pi prompt', () => {
  test.use({ pausedClock: true });

  test('Find remains unavailable and preserves a typed prompt answer', async ({
    page,
  }) => {
    await page.goto('/?test-scenario=saved-session-extension-input-navigation');
    const input = page.getByRole('textbox', { name: 'Release note label' });
    await expect(input).toBeFocused();
    await input.fill('Unsubmitted rc.2 answer');
    await shortcut(page, 'f');
    await expect(searchBar(page)).toHaveCount(0);
    await expect(input).toBeFocused();
    await expect(input).toHaveValue('Unsubmitted rc.2 answer');
    await shortcut(page, 'k');
    const palette = page.getByRole('dialog', {
      name: 'Command Palette',
      exact: true,
    });
    await expect(palette).toBeVisible();
    await palette.getByRole('combobox').fill('Find in Transcript');
    await expect(
      palette.getByRole('option', { name: /Find in Transcript/ }),
    ).toHaveCount(0);
    await palette.getByRole('combobox').press('Escape');
    await expect(input).toBeFocused();
    await expect(input).toHaveValue('Unsubmitted rc.2 answer');

    // Complete the scenario's expected session navigation, preserving the answer.
    await shortcut(page, 'k');
    await palette.getByRole('combobox').fill('New Session');
    await palette.getByRole('option', { name: /New Session/ }).click();
    await expect(
      page.getByRole('heading', { name: 'New Session', exact: true }),
    ).toBeVisible();
    await shortcut(page, 'p');
    await page
      .getByRole('dialog', { name: 'Switch Session' })
      .getByRole('option', { name: /^Main(?:\s|$)/ })
      .click();
    await expect(input).toBeFocused();
    await expect(input).toHaveValue('Unsubmitted rc.2 answer');
  });
});

test('compacted search only adds earlier results after explicitly loading history', async ({
  page,
}) => {
  await page.goto(
    '/?fixture=long-transcript&search=true&count=12&compacted=true',
  );
  await expect(page.getByTestId('fixture-count')).toHaveText('13 messages');
  await expect(searchBar(page)).not.toContainText('Loaded messages only');
  await query(page, 'Search checkpoint', '1 / 3');
  await expectActiveRange(page, 'Search checkpoint', 'current-search-early');
  await searchInput(page).press('Shift+Enter');
  await expectActiveRange(page, 'Search checkpoint', 'current-search-late');
  await searchInput(page).press('Enter');
  await expectActiveRange(page, 'Search checkpoint', 'current-search-early');
  await expect(page.getByTestId('fixture-count')).toHaveText('13 messages');
  await expect(
    page.getByRole('button', { name: 'Load earlier messages', exact: true }),
  ).toBeEnabled();
  await page
    .getByRole('button', { name: 'Load earlier messages', exact: true })
    .click();
  await expect(page.getByTestId('fixture-count')).toHaveText('29 messages');
  await expect(searchBar(page)).toContainText('2 / 4');
  await searchInput(page).focus();
  await searchInput(page).press('Shift+Enter');
  await expect(searchBar(page)).toContainText('1 / 4');
  await expectActiveRange(page, 'Search checkpoint', 'history-fixture-user-0');
});

for (const { container, initial, target, count } of [
  {
    container: 'pre',
    initial: 'const codeNeedle',
    target: 'horizontal-search-target',
    count: '1 / 3',
  },
  {
    container: 'table',
    initial: 'Column 0',
    target: 'table-search-target',
    count: '1 / 1',
  },
]) {
  test(`a horizontally clipped ${container} match scrolls its container rather than the app`, async ({
    page,
  }) => {
    await page.goto('/?fixture=long-transcript&search=true&count=12');
    await query(page, initial, container === 'pre' ? '1 / 3' : '1 / 1');
    await expectActiveRange(page, initial, 'search-early');
    const pre = page.locator(`[data-message-id="search-early"] ${container}`);
    await expect
      .poll(() =>
        pre.evaluate((element) => element.scrollWidth - element.clientWidth),
      )
      .toBeGreaterThan(500);
    expect(await pre.evaluate((element) => element.scrollLeft)).toBe(0);
    const before = await page.evaluate(() => ({
      x: window.scrollX,
      left: document
        .querySelector('[aria-label="Transcript"]')!
        .getBoundingClientRect().left,
    }));
    await query(page, target, count);
    await expectActiveRange(page, target, 'search-early');
    await expect(searchInput(page)).toBeFocused();
    await expect
      .poll(() => pre.evaluate((element) => element.scrollLeft))
      .toBeGreaterThan(500);
    expect(
      await page
        .getByLabel('Transcript', { exact: true })
        .evaluate((element) => element.scrollLeft),
    ).toBe(0);
    expect(
      await page.evaluate(() => ({
        x: window.scrollX,
        left: document
          .querySelector('[aria-label="Transcript"]')!
          .getBoundingClientRect().left,
      })),
    ).toEqual(before);
    await expect
      .poll(() =>
        page.evaluate((selector) => {
          const range = [
            ...CSS.highlights!.get('tau-transcript-active')!,
          ][0] as Range;
          const rect = range.getBoundingClientRect();
          const box = range.startContainer
            .parentElement!.closest(selector)!
            .getBoundingClientRect();
          return rect.left >= box.left && rect.right <= box.right;
        }, container),
      )
      .toBe(true);
  });
}

test('closing Find at a historical match preserves the reader position through streaming', async ({
  page,
}) => {
  await page.goto('/?fixture=long-transcript&search=true');
  await query(page, 'Search checkpoint', '1 / 3');
  await searchInput(page).press('Enter');
  await expectActiveRange(page, 'Search checkpoint', 'search-middle');
  await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(6));
  await expectActiveRange(page, 'Search checkpoint', 'search-middle');
  const transcript = page.getByLabel('Transcript', { exact: true });
  const row = page.locator('[data-message-id="search-middle"]');
  const before = await row.evaluate(
    (element) => element.getBoundingClientRect().top,
  );
  const top = await transcript.evaluate((element) => element.scrollTop);
  await page.getByRole('button', { name: 'Close Find', exact: true }).click();
  await expect(searchBar(page)).toHaveCount(0);
  await expect
    .poll(() =>
      row.evaluate(
        (element, y) => Math.abs(element.getBoundingClientRect().top - y),
        before,
      ),
    )
    .toBeLessThan(2);
  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.streamLatest(18),
  );
  await expect
    .poll(() =>
      row.evaluate(
        (element, y) => Math.abs(element.getBoundingClientRect().top - y),
        before,
      ),
    )
    .toBeLessThan(2);
  await expect
    .poll(() =>
      transcript.evaluate((element, y) => Math.abs(element.scrollTop - y), top),
    )
    .toBeLessThan(2);
});

test('at 480px a long title leaves Find controls reachable and the active range below the overlay', async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 720 });
  await page.goto('/');
  const composer = page.getByRole('textbox', {
    name: 'Message Pi',
    exact: true,
  });
  await expect(composer).toBeEnabled();
  await composer.fill('Narrow window draft');
  await page
    .getByRole('button', { name: 'Workspace overview', exact: true })
    .click();
  const title =
    'A very long session title that must not crowd out transcript search controls '.repeat(
      3,
    );
  const name = page.getByRole('textbox', { name: 'Session Name', exact: true });
  await name.fill(title);
  await name.press('Enter');
  await expect(
    page.getByRole('heading', { name: title.trim(), exact: true }),
  ).toBeVisible();
  await shortcut(page, 'f');
  await query(page, 'Atlas', '1 / 2');
  await expectActiveRange(page, 'Atlas');
  await expect(searchInput(page)).toBeInViewport({ ratio: 1 });
  await searchInput(page).click({ trial: true });
  const activeBelowOverlay = (): Promise<boolean> =>
    page.evaluate(() => {
      const range = [
        ...CSS.highlights!.get('tau-transcript-active')!,
      ][0] as Range;
      const overlay = document
        .querySelector('[role="search"][aria-label="Find in Transcript"]')!
        .getBoundingClientRect();
      return range.getBoundingClientRect().top >= overlay.bottom;
    });
  await expect.soft.poll(activeBelowOverlay).toBe(true);
  for (const label of ['Next Match', 'Previous Match', 'Close Find']) {
    const button = page.getByRole('button', { name: label, exact: true });
    await expect(button).toBeInViewport({ ratio: 1 });
    await button.click({ trial: true });
  }
  await page.getByRole('button', { name: 'Next Match', exact: true }).click();
  await expect(searchBar(page)).toContainText('2 / 2');
  await expectActiveRange(page, 'Atlas');
  await expect.poll(activeBelowOverlay).toBe(true);
  await page
    .getByRole('button', { name: 'Previous Match', exact: true })
    .click();
  await expect(searchBar(page)).toContainText('1 / 2');
  await page.getByRole('button', { name: 'Close Find', exact: true }).click();
  await expect(composer).toHaveValue('Narrow window draft');
});

for (const width of [480, 1280]) {
  for (const colorScheme of ['light', 'dark'] as const) {
    test(`compact search field contains its status at ${width}px in ${colorScheme} mode`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme });
      await page.goto('/?fixture=long-transcript&search=true&count=12');
      await expect(searchInput(page)).toHaveAttribute('placeholder', 'Search');
      await expect(searchBar(page)).not.toContainText('Loaded messages only');
      const geometry = async (): Promise<void> => {
        const boxes = await searchBar(page).evaluate((popup) => {
          const field = popup.querySelector('.search-field')!;
          const input = field.querySelector('input')!;
          const status = field.querySelector('[aria-live]')!;
          const controls = popup.querySelector('.search-controls')!;
          const buttons = [...popup.querySelectorAll('button')];
          const rect = (
            element: Element,
          ): { left: number; right: number; width: number } => {
            const { left, right, width } = element.getBoundingClientRect();
            return { left, right, width };
          };
          return {
            popup: rect(popup),
            field: rect(field),
            input: rect(input),
            status: rect(status),
            controls: rect(controls),
            buttons: buttons.map(rect),
            inputAlign: getComputedStyle(input).textAlign,
            statusAlign: getComputedStyle(status).textAlign,
          };
        });
        expect(boxes.popup.left).toBeGreaterThanOrEqual(0);
        expect(boxes.popup.right).toBeLessThanOrEqual(width);
        expect(boxes.field.left).toBeGreaterThanOrEqual(boxes.controls.left);
        expect(boxes.field.left - boxes.controls.left).toBeLessThan(10);
        expect(boxes.field.right).toBeLessThanOrEqual(boxes.buttons[0]!.left);
        expect(boxes.buttons[0]!.left - boxes.field.right).toBeLessThan(5);
        expect(boxes.input.width).toBeGreaterThan(35);
        expect(boxes.input.left).toBeGreaterThanOrEqual(boxes.field.left);
        expect(boxes.input.right).toBeLessThanOrEqual(boxes.status.left);
        expect(boxes.status.right).toBeLessThanOrEqual(boxes.field.right);
        expect(boxes.inputAlign).toMatch(/^(start|left)$/);
        expect(boxes.statusAlign).toBe('right');
        for (const label of ['Previous Match', 'Next Match', 'Close Find']) {
          await expect(
            page.getByRole('button', { name: label, exact: true }),
          ).toBeInViewport({ ratio: 1 });
        }
      };
      await query(page, '', '');
      await geometry();
      await query(page, 'horizontal padding', '1 / 120');
      await searchInput(page).press('Shift+Enter');
      await expect(searchBar(page)).toContainText('120 / 120');
      await geometry();
      const before = await page
        .getByRole('button', { name: 'Close Find', exact: true })
        .boundingBox();
      await query(
        page,
        'A very long unmatched query '.repeat(20),
        'No matches',
      );
      await geometry();
      await expect(searchInput(page)).toBeFocused();
      expect(
        await page
          .getByRole('button', { name: 'Close Find', exact: true })
          .boundingBox(),
      ).toEqual(before);
      await query(page, '', '');
      await geometry();
    });
  }
}

test('first-match clearance survives closing Find and switching sessions without moving the reader', async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 720 });
  await page.goto('/');
  await expect(
    page.getByRole('textbox', { name: 'Message Pi', exact: true }),
  ).toBeEnabled();
  const transcript = page.getByLabel('Transcript', { exact: true });
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  const firstRow = transcript.locator('[data-message-id]').first();
  const rowTop = (): Promise<number> =>
    firstRow.evaluate((element) => element.getBoundingClientRect().top);
  const initial = await rowTop();
  await shortcut(page, 'f');
  await expect(searchInput(page)).toBeFocused();
  expect(await rowTop()).toBe(initial);
  await query(page, 'Atlas', '1 / 2');
  await expectActiveRange(page, 'Atlas');
  await expect.poll(rowTop).toBeGreaterThan(initial);
  const revealed = await rowTop();
  await page.getByRole('button', { name: 'Close Find', exact: true }).click();
  await expect(searchBar(page)).toHaveCount(0);
  expect(Math.abs((await rowTop()) - revealed)).toBeLessThan(2);
  const sidebar = page.getByRole('complementary', {
    name: 'Projects and Sessions',
  });
  await sidebar.getByRole('button', { name: /^Navigation review / }).click();
  await sidebar.getByRole('button', { name: /^Workspace overview / }).click();
  await expect(searchBar(page)).toHaveCount(0);
  await expect
    .poll(async () => Math.abs((await rowTop()) - revealed))
    .toBeLessThan(2);
  await shortcut(page, 'f');
  await expect(searchInput(page)).toHaveValue('Atlas');
  expect(Math.abs((await rowTop()) - revealed)).toBeLessThan(2);
});
