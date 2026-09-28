import { expect, test } from './fixtures';

const source =
  `/* opening comment\ncontinued comment */\nconst value: number = 42;\n${'export const item = value;\n'.repeat(850)}// end marker\n`.replaceAll(
    '\n',
    '\r\n',
  );

test('colors source past the transcript cutoff in both appearances', async ({
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
  for (const appearance of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: appearance });
    await expect
      .poll(() =>
        preview
          .locator('.shiki span[style]')
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
  await page.keyboard.press('Meta+A');
  expect(
    (await page.evaluate(() => window.getSelection()?.toString()))?.trimEnd(),
  ).toBe(source.trimEnd());
});
