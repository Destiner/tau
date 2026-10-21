import { expect, test } from './fixtures';

const samples = [
  { filename: 'example.ts', source: 'const answer: number = 42; // note\n' },
  {
    filename: 'example.vue',
    source:
      '<template><div>{{ message }}</div></template>\n<script setup lang="ts">const message: string = "hello";</script>\n',
  },
  {
    filename: 'example.rs',
    source: 'fn main() { let answer: i32 = 42; // note\n}\n',
  },
  { filename: 'example.json', source: '{"answer": 42, "ready": true}\n' },
] as const;

test('short source grammars have themed token colors', async ({ page }) => {
  await page.addInitScript((files) => {
    let index = 0;
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        transformCallback: (callback: unknown): unknown => callback,
        invoke: (command: string): Promise<unknown> => {
          if (command !== 'prepare_file_preview') return Promise.resolve(null);
          const file = files[index++ % files.length]!;
          return Promise.resolve({
            kind: 'ready',
            id: `sample-${index}`,
            ...file,
            sourcePath: `/project/${file.filename}`,
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(file.source)}`,
            byteLength: file.source.length,
          });
        },
      },
    });
  }, samples);
  await page.goto('/?fixture=long-transcript&preview=true');
  const path = page.locator(
    '[data-message-id="fixture-markdown-showcase"] [data-tau-path="src/components/TranscriptView.vue"]',
  );
  for (const sample of samples) {
    await path.focus();
    await path.press('Enter');
    const preview = page.getByRole('dialog', { name: sample.filename });
    const colors: string[] = [];
    for (const appearance of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: appearance });
      await expect
        .poll(() =>
          preview
            .locator('.shiki span[style]')
            .evaluateAll(
              (nodes) =>
                new Set(nodes.map((node) => getComputedStyle(node).color)).size,
            ),
        )
        .toBeGreaterThan(1);
      colors.push(
        await preview
          .locator('.shiki span[style]')
          .first()
          .evaluate((node) => getComputedStyle(node).color),
      );
    }
    expect(colors[0]).not.toBe(colors[1]);
    await expect(preview.locator('.file-viewer-code')).toHaveText(
      sample.source.trimEnd(),
    );
    await page.keyboard.press('Escape');
    await expect(preview).toHaveCount(0);
  }
});
