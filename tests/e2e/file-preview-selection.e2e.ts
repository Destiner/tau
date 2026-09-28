import { expect, test } from './fixtures';

const source = `const token: number = 42;\n${'// a comment\n'.repeat(2_000)}`;

test('defers worker output during selection and discards output after close', async ({
  page,
}) => {
  await page.addInitScript((code) => {
    const NativeWorker = window.Worker;
    const held: Array<() => void> = [];
    const types: string[] = [];
    Object.assign(window, {
      __TAU_HELD_TYPES__: types,
      __TAU_RELEASE_HIGHLIGHT__: () => {
        for (const deliver of held.splice(0)) deliver();
      },
      __TAURI_INTERNALS__: {
        transformCallback: (callback: unknown): unknown => callback,
        invoke: (command: string): Promise<unknown> =>
          command === 'prepare_file_preview'
            ? Promise.resolve({
                kind: 'ready',
                id: 'selected',
                filename: 'TranscriptView.ts',
                sourcePath: '/project/TranscriptView.ts',
                assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(code)}`,
                byteLength: code.length,
              })
            : Promise.resolve(null),
      },
    });
    window.Worker = class extends NativeWorker {
      constructor(url: string | URL, options?: WorkerOptions) {
        super(url, options);
        let receiver: ((event: MessageEvent) => void) | null = null;
        Object.defineProperty(this, 'onmessage', {
          get: () => receiver,
          set: (handler: typeof receiver) => {
            receiver = handler;
          },
        });
        this.addEventListener('message', (event) => {
          types.push((event.data as { type: string }).type);
          held.push(() => receiver?.(event));
        });
      }
    };
  }, source);
  await page.goto('/?fixture=long-transcript&preview=true');
  const path = page.locator(
    '[data-message-id="fixture-markdown-showcase"] [data-tau-path="src/components/TranscriptView.vue"]',
  );
  await path.focus();
  await path.press('Enter');
  const preview = page.getByRole('dialog', { name: 'TranscriptView.ts' });
  await expect(preview.locator('.file-viewer-code')).toContainText('token');
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as Window & { __TAU_HELD_TYPES__?: string[] }
        ).__TAU_HELD_TYPES__?.includes('done'),
      ),
    )
    .toBe(true);
  await page.keyboard.press('Meta+A');
  const selected = await page.evaluate(() => window.getSelection()?.toString());
  expect(selected?.trimEnd()).toBe(source.trimEnd());
  await page.evaluate(() =>
    (
      window as Window & { __TAU_RELEASE_HIGHLIGHT__?: () => void }
    ).__TAU_RELEASE_HIGHLIGHT__?.(),
  );
  await expect(preview.locator('.shiki')).toHaveCount(0);
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe(
    selected,
  );
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(preview.locator('.shiki .line:last-child')).toHaveCount(1);
  await expect
    .poll(() => preview.locator('.file-viewer-code').textContent())
    .toBe(source);
  await page.keyboard.press('Escape');
  await expect(preview).toHaveCount(0);
  await path.press('Enter');
  await expect(preview).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as Window & { __TAU_HELD_TYPES__?: string[] }
          ).__TAU_HELD_TYPES__?.filter((type) => type === 'done').length,
      ),
    )
    .toBe(2);
  await page.keyboard.press('Escape');
  await page.evaluate(() =>
    (
      window as Window & { __TAU_RELEASE_HIGHLIGHT__?: () => void }
    ).__TAU_RELEASE_HIGHLIGHT__?.(),
  );
  await expect(preview).toHaveCount(0);
});
