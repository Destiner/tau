import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=extension-dialog';

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto(fixtureUrl);
});

test('opens the transcript at a question larger than the pane', async ({
  page,
}) => {
  const transcript = page.getByLabel('Transcript');
  const prompt = page.getByRole('dialog');
  await expect(prompt).toBeVisible();

  const transcriptBox = await transcript.boundingBox();
  const promptBox = await prompt.boundingBox();
  expect(transcriptBox).not.toBeNull();
  expect(promptBox).not.toBeNull();
  if (!transcriptBox || !promptBox) return;

  // The prompt is taller than the pane, and the transcript is scrolled to it:
  // its end is the end of the scroll region, and the question fills the view.
  expect(promptBox.height).toBeGreaterThan(transcriptBox.height);
  const scroll = await transcript.evaluate((element) => ({
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  expect(
    scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight,
  ).toBeLessThanOrEqual(2);
});

test('does not move a reader in history when a prompt appears', async ({
  page,
}) => {
  await page.goto(`${fixtureUrl}&delayed=true`);

  const transcript = page.getByLabel('Transcript');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect
    .poll(() =>
      transcript.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    )
    .toBe(true);
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
    element.dispatchEvent(new Event('scroll'));
  });
  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
  const before = await transcript.evaluate(transcriptSnapshot);
  expect(before).not.toBeNull();
  expect(before?.distanceFromEnd).toBeGreaterThan(100);

  await page.getByRole('button', { name: 'Show prompt' }).click();
  const prompt = transcript.getByRole('dialog');
  await expect(prompt).toBeVisible();
  await expect(prompt.getByRole('option').first()).toBeFocused();
  await waitForLayout(page);

  const after = await transcript.evaluate(transcriptSnapshot);
  expect(after).not.toBeNull();
  expect(after?.scrollHeight).toBeGreaterThan(before?.scrollHeight ?? 0);
  expect(after?.id).toBe(before?.id);
  expect(
    Math.abs((after?.offset ?? 0) - (before?.offset ?? 0)),
  ).toBeLessThanOrEqual(2);
  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
  expect(after?.distanceFromEnd).toBeGreaterThan(100);
});

test('keeps a reader at the end when a prompt appears', async ({ page }) => {
  await page.goto(`${fixtureUrl}&delayed=true`);

  const transcript = page.getByLabel('Transcript');
  await expect
    .poll(() => transcript.evaluate(distanceFromEnd))
    .toBeLessThanOrEqual(2);
  const heightBefore = await transcript.evaluate(
    (element) => element.scrollHeight,
  );

  await page.getByRole('button', { name: 'Show prompt' }).click();
  const prompt = transcript.getByRole('dialog');
  await expect(prompt).toBeVisible();
  await expect(prompt.getByRole('option').first()).toBeFocused();
  await waitForLayout(page);

  expect(
    await transcript.evaluate((element) => element.scrollHeight),
  ).toBeGreaterThan(heightBefore);
  expect(await transcript.evaluate(distanceFromEnd)).toBeLessThanOrEqual(2);
});

test('scrolls the question, its options, and the transcript as one region', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const options = prompt.getByRole('listbox');
  const transcript = page.getByLabel('Transcript');

  // Only the transcript scrolls: nothing inside the prompt is a region of
  // its own, so the question, the options, and the history read as one view.
  for (const region of [prompt, options]) {
    const overflow = await region.evaluate((element) => ({
      scrollHeight: element.scrollHeight,
      clientHeight: element.clientHeight,
    }));
    expect(overflow.scrollHeight).toBe(overflow.clientHeight);
  }

  const transcriptOverflow = await transcript.evaluate((element) => ({
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  expect(transcriptOverflow.scrollHeight).toBeGreaterThan(
    transcriptOverflow.clientHeight,
  );

  // The messages that came before the question are still reachable above it.
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
});

test('preserves block markdown carried in the dialog title', async ({
  page,
}) => {
  const title = page.locator('.extension-prompt-title');

  await expect(title.locator('p')).toHaveCount(3);
  await expect(title.locator('.code-block pre code')).toHaveText(
    "const label = 'release';",
  );
  await expect(
    title.getByRole('link', {
      name: '/home/agent/.pi/workflows/implement/RHI-6283/implementation-plan.md',
    }),
  ).toBeVisible();
});

test('copies an absolute path from a remote dialog title', async ({ page }) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    (
      window as typeof window & {
        __TAU_CLIPBOARD_WRITES__?: string[];
        __TAURI_INTERNALS__?: unknown;
      }
    ).__TAU_CLIPBOARD_WRITES__ = writes;
    (
      window as typeof window & {
        __TAURI_INTERNALS__?: unknown;
      }
    ).__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const plan = page.getByRole('button', {
    name: 'Preview path /home/agent/.pi/workflows/implement/RHI-6283/implementation-plan.md',
  });
  await expect(plan).toBeVisible();
  await plan.click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as typeof window & {
              __TAU_CLIPBOARD_WRITES__?: string[];
            }
          ).__TAU_CLIPBOARD_WRITES__,
      ),
    )
    .toEqual([
      '/home/agent/.pi/workflows/implement/RHI-6283/implementation-plan.md',
    ]);
});

test('renders a table in the question as a table', async ({ page }) => {
  const prompt = page.getByRole('dialog');
  const header = prompt.locator('.extension-prompt-message th').first();
  await expect(header).toBeVisible();

  const style = await header.evaluate((element) => {
    const computed = window.getComputedStyle(element);
    return {
      // Rows are ruled, columns are not: the header's rule is its underline.
      rule: Number.parseFloat(computed.borderBottomWidth),
      columnRule: Number.parseFloat(computed.borderRightWidth),
      padding: Number.parseFloat(computed.paddingLeft),
      background: computed.backgroundColor,
      textAlign: computed.textAlign,
    };
  });
  expect(style.rule).toBeGreaterThan(0);
  expect(style.columnRule).toBe(0);
  expect(style.padding).toBeGreaterThan(0);
  expect(style.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(style.textAlign).toBe('left');
});

test('scrolls a table wider than the question inside itself', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const table = prompt.locator('.extension-prompt-message table');

  const width = await table.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(width.scrollWidth).toBeGreaterThan(width.clientWidth);

  // The table takes the sideways scrolling, so the prompt keeps its own width.
  const promptWidth = await prompt.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(promptWidth.scrollWidth).toBe(promptWidth.clientWidth);
});

test('reaches an option below the fold and reports the choice', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const option = prompt.getByRole('option', { name: 'label-11' });

  await option.scrollIntoViewIfNeeded();
  await option.click();

  await expect(page.getByTestId('dialog-outcome')).toHaveText(
    '{"submit":"label-11"}',
  );
  await expect(prompt).toBeHidden();
});

for (const method of ['select', 'confirm', 'input', 'editor'] as const) {
  test(`Escape cancels a focused ${method} prompt without bubbling`, async ({
    page,
  }) => {
    await page.goto(`${fixtureUrl}&method=${method}&draft=unfinished`);
    const prompt = page.getByRole('dialog');
    const control =
      method === 'select'
        ? prompt.getByRole('option').first()
        : method === 'confirm'
          ? prompt.getByRole('button', { name: 'Confirm' })
          : prompt.getByRole('textbox');
    await expect(control).toBeFocused();

    const result = await control.evaluate((element) => {
      const observed = { document: 0, window: 0 };
      document.addEventListener(
        'keydown',
        () => {
          observed.document += 1;
        },
        { once: true },
      );
      window.addEventListener(
        'keydown',
        () => {
          observed.window += 1;
        },
        { once: true },
      );
      const event = new KeyboardEvent('keydown', {
        key: 'Escape',
        bubbles: true,
        cancelable: true,
      });
      element.dispatchEvent(event);
      return { defaultPrevented: event.defaultPrevented, observed };
    });

    expect(result).toEqual({
      defaultPrevented: true,
      observed: { document: 0, window: 0 },
    });
    await expect(page.getByTestId('dialog-outcome')).toHaveText(
      '{"cancel":true,"count":1}',
    );
    await expect(prompt).toHaveCount(0);
  });
}

test('consumes Escape while a prompt is submitting or disabled', async ({
  page,
}) => {
  for (const state of ['submitting', 'disabled'] as const) {
    await page.goto(`${fixtureUrl}&method=input&${state}=true`);
    const prompt = page.getByRole('dialog');
    const result = await prompt.evaluate((element) => {
      const event = new KeyboardEvent('keydown', {
        key: 'Escape',
        bubbles: true,
        cancelable: true,
      });
      element.dispatchEvent(event);
      return event.defaultPrevented;
    });

    expect(result).toBe(true);
    await expect(prompt).toBeVisible();
    await expect(page.getByTestId('dialog-outcome')).toBeEmpty();
  }
});

test('ignores composition Escape and claims repeated cancellation once', async ({
  page,
}) => {
  await page.goto(`${fixtureUrl}&method=input&draft=unfinished`);
  const input = page.getByRole('dialog').getByRole('textbox');

  const composingPrevented = await input.evaluate((element) => {
    const event = new KeyboardEvent('keydown', {
      key: 'Escape',
      bubbles: true,
      cancelable: true,
      isComposing: true,
    });
    element.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(composingPrevented).toBe(false);
  await expect(page.getByRole('dialog')).toBeVisible();

  await input.evaluate((element) => {
    for (const repeat of [false, true]) {
      element.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'Escape',
          bubbles: true,
          cancelable: true,
          repeat,
        }),
      );
    }
  });
  await expect(page.getByTestId('dialog-outcome')).toHaveText(
    '{"cancel":true,"count":1}',
  );
});

test('a context menu above the prompt owns the first Escape', async ({
  page,
}) => {
  await page.goto(`${fixtureUrl}&method=input&draft=unfinished`);
  const prompt = page.getByRole('dialog');
  const input = prompt.getByRole('textbox');
  await input.click({ button: 'right' });
  await expect(page.getByRole('menu')).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).toHaveCount(0);
  await expect(prompt).toBeVisible();
  await expect(input).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(prompt).toHaveCount(0);
});

function distanceFromEnd(element: HTMLElement): number {
  return element.scrollHeight - element.scrollTop - element.clientHeight;
}

function transcriptSnapshot(element: HTMLElement): {
  id: string;
  offset: number;
  scrollHeight: number;
  distanceFromEnd: number;
} | null {
  const viewport = element.getBoundingClientRect();
  const anchor = Array.from(
    element.querySelectorAll<HTMLElement>('[data-message-id]'),
  ).find((row) => row.getBoundingClientRect().bottom > viewport.top);
  if (!anchor) return null;

  return {
    id: anchor.dataset.messageId ?? '',
    offset: anchor.getBoundingClientRect().top - viewport.top,
    scrollHeight: element.scrollHeight,
    distanceFromEnd:
      element.scrollHeight - element.scrollTop - element.clientHeight,
  };
}

async function waitForLayout(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
  );
}
