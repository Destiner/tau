import type { Locator, Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&geometry=true';
const tolerance = 1;

type Rails = { left: number; right: number; width: number };

async function rails(locator: Locator): Promise<Rails> {
  return locator.evaluate((element) => {
    const { left, right, width } = element.getBoundingClientRect();
    return { left, right, width };
  });
}

function sameRails(actual: Rails, expected: Rails): void {
  expect(Math.abs(actual.left - expected.left)).toBeLessThanOrEqual(tolerance);
  expect(Math.abs(actual.right - expected.right)).toBeLessThanOrEqual(
    tolerance,
  );
}

function contained(actual: Rails, parent: Rails): void {
  expect(actual.left).toBeGreaterThanOrEqual(parent.left - tolerance);
  expect(actual.right).toBeLessThanOrEqual(parent.right + tolerance);
}

// Rows outside the virtual window do not exist. Walk overlapping viewports,
// allowing Vue and ResizeObserver to measure each window before moving on.
async function reveal(page: Page, id: string): Promise<Locator> {
  await page.locator('.transcript').evaluate(async (element, messageId) => {
    const frame = (): Promise<void> =>
      new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    element.scrollTop = 0;
    await frame();
    await frame();
    for (let step = 0; step < 160; step += 1) {
      const row = element.querySelector<HTMLElement>(
        `[data-message-id="${messageId}"]`,
      );
      if (row) {
        row.scrollIntoView({ block: 'start' });
        await frame();
        await frame();
        return;
      }
      element.scrollTop += element.clientHeight / 2;
      await frame();
      await frame();
    }
    throw new Error(`Could not reveal virtual transcript row ${messageId}`);
  }, id);
  const row = page.locator(`[data-message-id="${id}"]`);
  await expect(row).toBeVisible();
  return row;
}

async function assertNoPageOverflow(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => {
    const transcript = document.querySelector<HTMLElement>('.transcript')!;
    return {
      page:
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
      transcript: transcript.scrollWidth - transcript.clientWidth,
    };
  });
  expect(overflow.page).toBeLessThanOrEqual(tolerance);
  expect(overflow.transcript).toBeLessThanOrEqual(tolerance);
}

async function assertLocalScroll(locator: Locator): Promise<void> {
  await expect
    .poll(() =>
      locator.evaluate((element) => element.scrollWidth - element.clientWidth),
    )
    .toBeGreaterThan(1);
  const before = await rails(locator);
  const offset = await locator.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
    return element.scrollLeft;
  });
  expect(offset).toBeGreaterThan(0);
  sameRails(await rails(locator), before);
}

for (const width of [390]) {
  test.describe(`transcript geometry at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 } });

    test.beforeEach(async ({ page }) => {
      await page.goto(fixtureUrl);
      await expect(page.getByTestId('fixture-count')).toHaveText('18 messages');
    });

    test('shares user and assistant prose and top-level structured-content rails', async ({
      page,
    }) => {
      let prose: Rails | undefined;
      let outer: Rails | undefined;
      for (const role of ['user', 'assistant']) {
        const row = await reveal(page, `geometry-${role}-prose`);
        const current = await rails(row.locator('.markdown > h1'));
        const currentOuter = await rails(row);
        if (prose && outer) {
          sameRails(current, prose);
          sameRails(currentOuter, outer);
        }
        prose = current;
        outer = currentOuter;
        contained(current, currentOuter);
        expect(current.left).toBeGreaterThan(currentOuter.left);
        expect(current.right).toBeLessThan(currentOuter.right);
        sameRails(await rails(row.locator('.markdown > p').first()), current);
        sameRails(await rails(row.locator('.markdown > h2')), current);
        sameRails(await rails(row.locator('.markdown > hr')), current);
        sameRails(await rails(row.locator('.markdown > blockquote')), current);
        const lists = await reveal(page, `geometry-${role}-lists`);
        for (const list of await lists
          .locator('.markdown > :is(ul, ol)')
          .all()) {
          sameRails(await rails(list), current);
        }
        expect(
          Math.abs((currentOuter.left + currentOuter.right) / 2 - width / 2),
        ).toBeLessThanOrEqual(tolerance);

        const code = await reveal(page, `geometry-${role}-code`);
        sameRails(await rails(code.locator('.markdown > h2')), current);
        for (const block of await code
          .locator('.markdown > .code-block')
          .all()) {
          sameRails(await rails(block), currentOuter);
          const contentLeft = await block.locator('pre').evaluate((element) => {
            const style = getComputedStyle(element);
            return (
              element.getBoundingClientRect().left +
              parseFloat(style.borderLeftWidth) +
              parseFloat(style.paddingLeft)
            );
          });
          expect(Math.abs(contentLeft - current.left)).toBeLessThanOrEqual(
            tolerance,
          );
        }
        await assertLocalScroll(code.locator('pre').last());

        const diagrams = await reveal(page, `geometry-${role}-mermaid`);
        await expect(diagrams.locator('.diagram > svg')).toHaveCount(2);
        for (const diagram of await diagrams
          .locator('.markdown > .diagram')
          .all()) {
          sameRails(await rails(diagram), currentOuter);
          contained(await rails(diagram.locator('svg').first()), currentOuter);
        }

        const tables = await reveal(page, `geometry-${role}-tables`);
        const smallTable = await rails(tables.locator('table').first());
        expect(
          Math.abs(smallTable.left - currentOuter.left),
        ).toBeLessThanOrEqual(tolerance);
        expect(smallTable.width).toBeLessThan(currentOuter.width);
        const wideTable = tables.locator('table').last();
        expect(
          Math.abs((await rails(wideTable)).left - currentOuter.left),
        ).toBeLessThanOrEqual(tolerance);
        contained(await rails(wideTable), currentOuter);
        await assertLocalScroll(wideTable);
        await assertNoPageOverflow(page);
      }
    });

    test('preserves natural image dimensions and contains nested structured content', async ({
      page,
    }) => {
      for (const role of ['user', 'assistant']) {
        const row = await reveal(page, `geometry-${role}-images`);
        const images = row.locator('img');
        await expect(images).toHaveCount(2);
        await expect
          .poll(() =>
            images.evaluateAll((elements) =>
              elements.every(
                (element) =>
                  (element as HTMLImageElement).complete &&
                  (element as HTMLImageElement).naturalWidth > 0,
              ),
            ),
          )
          .toBe(true);
        const small = images.first();
        const large = images.last();
        const natural = await small.evaluate((element) => ({
          width: (element as HTMLImageElement).naturalWidth,
          height: (element as HTMLImageElement).naturalHeight,
        }));
        const box = await small.boundingBox();
        expect(box!.width).toBe(natural.width);
        expect(box!.height).toBe(natural.height);
        const prose = await rails(row.locator('.markdown > h2'));
        expect(
          Math.abs((await rails(small)).left - prose.left),
        ).toBeLessThanOrEqual(tolerance);
        sameRails(await rails(large), prose);
        const largeBox = await large.boundingBox();
        expect(
          Math.abs(largeBox!.width / largeBox!.height - 1600 / 180),
        ).toBeLessThan(0.02);
        await assertNoPageOverflow(page);
      }

      for (const [id, container] of [
        ['geometry-user-structured-list', 'li'],
        ['geometry-assistant-structured-quote', 'blockquote'],
      ] as const) {
        const row = await reveal(page, id);
        await expect(row.locator('.diagram > svg')).toHaveCount(1);
        for (const child of await row
          .locator('.code-block, .diagram, table, img')
          .all()) {
          const parent = await child.evaluate((element, selector) => {
            const ancestor = element.closest(selector)!;
            const { left, right, width } = ancestor.getBoundingClientRect();
            return { left, right, width };
          }, container);
          contained(await rails(child), parent);
        }
        await assertNoPageOverflow(page);
      }
    });
  });
}

test('keeps rows disjoint and the end anchored when the viewport narrows', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('18 messages');
  await reveal(page, 'geometry-compaction-boundary');
  await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__!.scrollToEnd());
  await expect.poll(() => endGap(page)).toBeLessThanOrEqual(2);
  await page.setViewportSize({ width: 390, height: 700 });
  await expect.poll(() => endGap(page)).toBeLessThanOrEqual(2);
  await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__!.appendMessage());
  await page.evaluate(() => window.__TAU_TRANSCRIPT_FIXTURE__!.streamLatest(6));
  await expect.poll(() => endGap(page)).toBeLessThanOrEqual(2);
  for (const id of [
    'geometry-user-prose',
    'geometry-assistant-code',
    'geometry-user-mermaid',
    'geometry-assistant-images',
    'geometry-assistant-structured-quote',
  ]) {
    await reveal(page, id);
    const overlaps = await page
      .locator('[data-message-id]')
      .evaluateAll((rows) =>
        rows
          .slice(1)
          .map(
            (row, index) =>
              rows[index]!.getBoundingClientRect().bottom -
              row.getBoundingClientRect().top,
          ),
      );
    for (const overlap of overlaps)
      expect(overlap).toBeLessThanOrEqual(tolerance);
  }
  await assertNoPageOverflow(page);
});

async function endGap(page: Page): Promise<number> {
  return page
    .locator('.transcript')
    .evaluate((element) =>
      Math.abs(element.scrollHeight - element.clientHeight - element.scrollTop),
    );
}

async function updateMarkdown(
  page: Page,
  id: string,
  text: string,
): Promise<void> {
  await page.evaluate(
    ({ id, text }) => {
      window.__TAU_TRANSCRIPT_FIXTURE__!.updateMessage(id, text);
    },
    { id, text },
  );
}

const streamingDiagram = [
  '## Streaming diagram\n\n```mermaid',
  '## Streaming diagram\n\n```mermaid\nflowchart TD\n  A[Start] --> B[Measure]',
  '## Streaming diagram\n\n```mermaid\nflowchart TD\n  A[Start] --> B[Measure]\n  B --> C[Render]\n  C --> D[Finish]\n```',
];

for (const width of [390]) {
  test.describe(`secondary and streaming geometry at ${width}px`, () => {
    test.use({ viewport: { width, height: 900 } });

    test.beforeEach(async ({ page }) => {
      await page.goto(fixtureUrl);
      await expect(page.getByTestId('fixture-count')).toHaveText('18 messages');
    });

    test('aligns secondary rows and the working indicator to prose rails', async ({
      page,
    }) => {
      const prose = await rails(
        (await reveal(page, 'geometry-assistant-prose')).locator(
          '.markdown > h1',
        ),
      );
      for (const id of [
        'geometry-tool-secondary-read',
        'geometry-notice-secondary',
        'geometry-error-secondary',
        'geometry-compaction-boundary',
      ]) {
        const row = await reveal(page, id);
        sameRails(await rails(row), prose);
        if (id === 'geometry-tool-secondary-read') {
          await row.getByRole('button').click();
          await expect(row.locator('.activity-details')).toBeVisible();
          contained(await rails(row.locator('.activity-details')), prose);
        }
        await assertNoPageOverflow(page);
      }
      await page.evaluate(() => {
        window.__TAU_TRANSCRIPT_FIXTURE__!.setWorking(true);
        window.__TAU_TRANSCRIPT_FIXTURE__!.scrollToEnd();
      });
      const working = page.locator('.transcript-stream-state');
      await expect(working).toBeVisible();
      sameRails(await rails(working), prose);
      await expect.poll(() => endGap(page)).toBeLessThanOrEqual(2);
      await assertNoPageOverflow(page);
    });

    test('follows staged unfinished Mermaid through diagram rendering at the live end', async ({
      page,
    }) => {
      const id = await page.evaluate(() =>
        window.__TAU_TRANSCRIPT_FIXTURE__!.appendMessage(),
      );
      await page.evaluate(() =>
        window.__TAU_TRANSCRIPT_FIXTURE__!.scrollToEnd(),
      );
      const row = page.locator(`[data-message-id="${id}"]`);
      for (const [index, text] of streamingDiagram.entries()) {
        await updateMarkdown(page, id, text);
        await expect(row.locator('.markdown > h2')).toHaveText(
          'Streaming diagram',
        );
        if (index < 2) {
          await expect(row.locator('.code-block')).toHaveCount(1);
          await expect(row.locator('.diagram')).toHaveCount(0);
          sameRails(await rails(row.locator('.code-block')), await rails(row));
        } else {
          await expect(row.locator('.diagram > svg')).toHaveCount(1);
          await expect(row.locator('.code-block')).toHaveCount(0);
          sameRails(await rails(row.locator('.diagram')), await rails(row));
          contained(
            await rails(row.locator('.diagram > svg')),
            await rails(row),
          );
        }
        await expect.poll(() => endGap(page)).toBeLessThanOrEqual(2);
        await assertNoPageOverflow(page);
      }
    });

    test('keeps historical text selection and reader position during unrelated Mermaid output', async ({
      page,
    }) => {
      const id = await page.evaluate(() =>
        window.__TAU_TRANSCRIPT_FIXTURE__!.appendMessage(),
      );
      const history = await reveal(page, 'geometry-assistant-prose');
      const paragraph = history.locator('.markdown > p').first();
      const selected = await paragraph.evaluate((element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const selection = window.getSelection()!;
        selection.removeAllRanges();
        selection.addRange(range);
        return selection.toString();
      });
      expect(selected).toContain('Paragraphs should preserve');
      const before = await history.boundingBox();
      const scrollTop = await page
        .locator('.transcript')
        .evaluate((element) => element.scrollTop);
      for (const text of streamingDiagram) {
        await updateMarkdown(page, id, text);
        // Wait for Vue and measurement callbacks without scrolling the new row into view.
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() =>
                requestAnimationFrame(() => resolve()),
              ),
            ),
        );
        await expect
          .poll(() => page.evaluate(() => window.getSelection()?.toString()))
          .toBe(selected);
        await expect
          .poll(async () =>
            Math.abs((await history.boundingBox())!.y - before!.y),
          )
          .toBeLessThanOrEqual(tolerance);
        expect(
          Math.abs(
            (await page
              .locator('.transcript')
              .evaluate((element) => element.scrollTop)) - scrollTop,
          ),
        ).toBeLessThanOrEqual(tolerance);
      }
      // The completed source renders when the reader later chooses to visit it.
      const latest = await reveal(page, id);
      await expect(latest.locator('.diagram > svg')).toHaveCount(1);
      await assertNoPageOverflow(page);
    });
  });
}
