import { expect, test } from './fixtures';

const source =
  `${'// comment ' + 'x'.repeat(480) + '\n'}`.repeat(1_050) +
  'export const lastToken = 42; // <script>window.previewExecuted = true</script>\n';

test('colors the final line near the read limit without losing scroll or text', async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.addInitScript((code) => {
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        transformCallback: (callback: unknown): unknown => callback,
        invoke: (command: string): Promise<unknown> =>
          command === 'prepare_file_preview'
            ? Promise.resolve({
                kind: 'ready',
                id: 'near-limit',
                filename: 'TranscriptView.ts',
                sourcePath: '/project/TranscriptView.ts',
                assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(code)}`,
                byteLength: code.length,
              })
            : Promise.resolve(null),
      },
    });
  }, source);
  await page.goto('/?fixture=long-transcript&preview=true');
  await page
    .locator(
      '[data-message-id="fixture-markdown-showcase"] [data-tau-path="src/components/TranscriptView.vue"]',
    )
    .click();
  const preview = page.getByRole('dialog', { name: 'TranscriptView.ts' });
  const content = preview.locator('.file-viewer-content');
  await expect(preview.locator('.file-viewer-code')).toContainText('lastToken');
  await content.evaluate((node) => node.scrollTo({ top: 500, left: 150 }));
  await expect
    .poll(() => preview.locator('.shiki .line:last-child').count())
    .toBe(1);
  await expect
    .poll(() => preview.locator('.file-viewer-code').textContent())
    .toBe(source);
  const scroll = await content.evaluate((node) => ({
    top: node.scrollTop,
    left: node.scrollLeft,
  }));
  expect(scroll.top).toBeGreaterThan(0);
  expect(scroll.left).toBeGreaterThan(0);
  await page.keyboard.press('End');
  await expect
    .poll(() =>
      content.evaluate(
        (node) => node.scrollTop + node.clientHeight >= node.scrollHeight - 2,
      ),
    )
    .toBe(true);
  await expect(preview.locator('script')).toHaveCount(0);
  await expect(
    preview
      .locator('.shiki .line')
      .filter({ hasText: 'lastToken' })
      .locator('span[style]')
      .first(),
  ).toBeVisible();
});
