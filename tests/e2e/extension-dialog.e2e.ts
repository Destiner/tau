import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=extension-dialog';

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
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

for (const state of ['submitting', 'disabled'] as const) {
  test(`consumes Escape while a prompt is ${state}`, async ({ page }) => {
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
  });
}

for (const [shortcut, expected] of [
  ['Meta+Enter', true],
  ['Control+Enter', true],
  ['Meta+Shift+Enter', false],
  ['Control+Shift+Enter', false],
] as const) {
  test(`responds to a confirm prompt once with ${shortcut}`, async ({
    page,
  }) => {
    await page.goto(`${fixtureUrl}&method=confirm`);

    const prompt = page.getByRole('dialog');
    await prompt.getByRole('button', { name: 'Confirm' }).press(shortcut);

    await expect(page.getByTestId('dialog-outcome')).toHaveText(
      JSON.stringify({ submit: expected }),
    );
    await expect(prompt).toHaveCount(0);
  });
}

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
