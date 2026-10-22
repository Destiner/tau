import { expect, test } from './fixtures';

const source =
  '/* <script>window.__TAU_PREVIEW_EXECUTED__ = true</script> */\r\n' +
  "const emoji = '🦊';\r" +
  'const\tvalue: number = 42;\n' +
  'export const item = value;\r\n'.repeat(850) +
  '// end marker <img src=x onerror="window.__TAU_PREVIEW_EXECUTED__ = true">\r\n';

test('colors and preserves exact mixed source past the transcript cutoff', async ({
  page,
}) => {
  await page.addInitScript((code) => {
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        transformCallback: (callback: unknown): unknown => callback,
        invoke: (command: string): Promise<unknown> =>
          command === 'prepare_file_preview'
            ? Promise.resolve({
                kind: 'ready',
                id: 'large-source',
                filename: 'TranscriptView.ts',
                sourcePath: '/project/TranscriptView.ts',
                assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(code)}`,
                byteLength: new TextEncoder().encode(code).length,
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
  for (const appearance of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: appearance });
    await expect
      .poll(() =>
        preview
          .locator('[data-tau-highlight] span[style]')
          .evaluateAll(
            (nodes) =>
              new Set(
                nodes.slice(0, 20).map((node) => getComputedStyle(node).color),
              ).size,
          ),
      )
      .toBeGreaterThan(1);
  }
  await expect(preview.locator('.file-viewer-code')).toContainText(
    '// end marker',
  );
  expect(await preview.locator('.file-viewer-code').textContent()).toBe(source);
  await expect(preview.locator('script, img')).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_EXECUTED__?: boolean })
          .__TAU_PREVIEW_EXECUTED__,
    ),
  ).not.toBe(true);
  await page.keyboard.press('Meta+A');
  expect(
    (await page.evaluate(() => window.getSelection()?.toString()))?.trimEnd(),
  ).toBe(source.trimEnd());
});
