import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

type Geometry = {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  list: { bottom: number; height: number };
  rows: { id: string; bottom: number }[];
  prompt: { top: number; bottom: number };
  actionHit: boolean;
};

async function geometry(page: Page): Promise<Geometry> {
  return page.getByLabel('Transcript').evaluate((element) => {
    const list = element.querySelector<HTMLElement>('.message-list')!;
    const windowElement =
      element.querySelector<HTMLElement>('.message-window')!;
    const prompt = element.querySelector<HTMLElement>('.message.prompt')!;
    const action = prompt.querySelector<HTMLElement>('button')!;
    const actionBox = action.getBoundingClientRect();
    const hit = document.elementFromPoint(
      actionBox.left + actionBox.width / 2,
      actionBox.top + actionBox.height / 2,
    );
    return {
      scrollTop: element.scrollTop,
      scrollHeight: element.scrollHeight,
      clientHeight: element.clientHeight,
      list: {
        bottom: list.getBoundingClientRect().bottom,
        height: list.getBoundingClientRect().height,
      },
      rows: Array.from(
        windowElement.querySelectorAll<HTMLElement>('[data-message-id]'),
      ).map((row) => ({
        id: row.dataset.messageId ?? '',
        bottom: row.getBoundingClientRect().bottom,
      })),
      prompt: {
        top: prompt.getBoundingClientRect().top,
        bottom: prompt.getBoundingClientRect().bottom,
      },
      actionHit: hit === action || action.contains(hit),
    };
  });
}

async function frame(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
  );
}

async function wheelTo(page: Page, direction: -1 | 1): Promise<void> {
  const pane = page.getByLabel('Transcript');
  const box = await pane.boundingBox();
  if (!box) throw new Error('Transcript has no bounds');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);

  let last: Geometry | undefined;
  for (let step = 0; step < 160; step++) {
    const before = await geometry(page);
    const limit = Math.max(0, before.scrollHeight - before.clientHeight);
    if (direction < 0 ? before.scrollTop <= 1 : limit - before.scrollTop <= 1)
      return;
    await page.mouse.wheel(0, direction * 420);
    await frame(page);
    const after = await geometry(page);
    last = after;
    expect(after.rows.length).toBeLessThan(50);
    expect(after.prompt.top).toBeGreaterThanOrEqual(after.list.bottom - 2);
    expect(after.list.height).toBeGreaterThan(0);
  }
  throw new Error(
    `Wheel never reached ${direction < 0 ? 'top' : 'bottom'}; last geometry: ${JSON.stringify(last)}`,
  );
}

test('tool disclosure and prompt draft survive wheel unmount and direct endpoint jumps', async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 480 });
  await page.goto(
    '/?fixture=extension-dialog&history=tools&prompt=compact&method=input',
  );
  const pane = page.getByLabel('Transcript');
  const input = page.getByRole('dialog').getByRole('textbox');
  await input.fill('A draft kept while reading');
  await input.evaluate((node: HTMLInputElement | HTMLTextAreaElement) => {
    node.setSelectionRange(2, 7);
  });
  const promptHandle = await page.locator('.message.prompt').elementHandle();
  await wheelTo(page, -1);
  const earlyTool = pane.locator('[data-message-id="tool-0-0"]');
  await earlyTool.locator('button').click();
  await expect(earlyTool.locator('.activity-details')).toBeVisible();
  await pane.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await frame(page);
  expect((await geometry(page)).rows.some((row) => row.id === 'tool-0-0')).toBe(
    false,
  );
  await pane.evaluate((node) => {
    node.scrollTop = 0;
  });
  await frame(page);
  await expect(
    pane.locator('[data-message-id="tool-0-0"] .activity-details'),
  ).toBeVisible();
  await wheelTo(page, 1);
  expect(
    await page
      .locator('.message.prompt')
      .evaluate((node, original) => node === original, promptHandle),
  ).toBe(true);
  await expect(input).toHaveValue('A draft kept while reading');
  expect(
    await input.evaluate((node: HTMLInputElement | HTMLTextAreaElement) => [
      node.selectionStart,
      node.selectionEnd,
    ]),
  ).toEqual([2, 7]);
  const end = await geometry(page);
  expect(end.prompt.top).toBeGreaterThanOrEqual(end.list.bottom - 2);
});

for (const promptSize of ['compact', 'tall'] as const) {
  test(`collapsed tool history stays in flow across wheel reversals with ${promptSize} approval`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 600 });
    await page.goto(
      `/?fixture=extension-dialog&history=tools&prompt=${promptSize}&method=select`,
    );
    const pane = page.getByLabel('Transcript');
    const prompt = page.getByRole('dialog');
    await expect(prompt).toBeVisible();
    await expect
      .poll(() =>
        pane.evaluate(
          (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
        ),
      )
      .toBeLessThanOrEqual(2);
    const initialPrompt = await prompt.elementHandle();
    for (let trip = 0; trip < 2; trip++) {
      await wheelTo(page, -1);
      await expect(pane.locator('[data-message-id="user-0"]')).toBeVisible();
      // Reverse before and after newly mounted rows settle.
      if (trip === 1) await frame(page);
      await wheelTo(page, 1);
      await frame(page);
      const end = await geometry(page);
      expect(
        end.scrollHeight - end.scrollTop - end.clientHeight,
      ).toBeLessThanOrEqual(2);
      expect(end.prompt.top).toBeGreaterThanOrEqual(end.list.bottom - 2);
      expect(end.rows.at(-1)?.bottom).toBeLessThanOrEqual(end.list.bottom + 2);
      if (promptSize === 'compact') expect(end.actionHit).toBe(true);
      expect(end.rows.some((row) => row.id === 'tool-0-0')).toBe(false);
      expect(
        await prompt.evaluate(
          (node, original) => node === original,
          initialPrompt,
        ),
      ).toBe(true);
    }
    await prompt.getByRole('option').last().scrollIntoViewIfNeeded();
    await prompt.getByRole('option').last().click();
    await expect(page.getByTestId('dialog-outcome')).toHaveText(
      `{"submit":"label-${promptSize === 'compact' ? 1 : 11}"}`,
    );
  });
}
