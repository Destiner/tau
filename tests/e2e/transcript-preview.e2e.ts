import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&preview=true';

declare global {
  interface Window {
    __TAU_CLIPBOARD_WRITES__?: string[];
    __TAU_OPENER_CALLS__?: string[];
    __TAURI_INTERNALS__?: {
      transformCallback: (callback: unknown) => unknown;
      invoke: (
        command: string,
        payload?: { text?: string },
      ) => Promise<unknown>;
    };
  }
}

test('previews local files and keeps URL and path copy actions', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    const openerCalls: string[] = [];
    const previewCalls: Array<{ command: string; payload?: unknown }> = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAU_OPENER_CALLS__ = openerCalls;
    Object.assign(window, { __TAU_PREVIEW_CALLS__: previewCalls });
    window.__TAURI_INTERNALS__ = {
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
        if (command.startsWith('plugin:opener|')) openerCalls.push(command);
        if (command === 'prepare_file_preview') {
          if (
            (payload as { path?: string } | undefined)?.path ===
            'src/components/'
          ) {
            return Promise.resolve({ kind: 'directory' });
          }
          previewCalls.push({ command, payload });
          const source = "export const component = 'TranscriptView';\n";
          return Promise.resolve({
            kind: 'ready',
            id: `local-${previewCalls.length}`,
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
            filename: 'TranscriptView.vue',
            sourcePath:
              '/Users/someone/code/tau/src/components/TranscriptView.vue',
            byteLength: source.length,
          });
        }
        if (command === 'release_file_preview') {
          previewCalls.push({ command, payload });
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(fixtureUrl);

  const message = page.locator('[data-message-id="fixture-markdown-showcase"]');
  const url = message.getByRole('link', { name: 'Tau docs' });
  const path = message.locator(
    '[data-tau-path="src/components/TranscriptView.vue"]',
  );

  await url.click({ button: 'right' });
  await expect(page.getByRole('menuitem')).toHaveText(['Copy URL']);
  await page.getByRole('menuitem', { name: 'Copy URL' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['https://example.com/tau/docs']);

  // The context menu augments rather than replaces the existing activation.
  await url.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_OPENER_CALLS__))
    .toEqual(['plugin:opener|open_url']);

  await path.click({ button: 'right' });
  await expect(page.getByRole('menuitem')).toHaveText([
    'Copy Path',
    'Copy Full Path',
  ]);
  await page.getByRole('menuitem', { name: 'Copy Path', exact: true }).click();
  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Full Path' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      'https://example.com/tau/docs',
      'src/components/TranscriptView.vue',
      '/Users/someone/code/tau/src/components/TranscriptView.vue',
    ]);

  await path.click();
  const preview = page.getByRole('dialog', {
    name: 'TranscriptView.vue',
  });
  await expect(preview).toBeVisible();
  await expect(preview.locator('.file-viewer-filename')).toHaveText(
    'TranscriptView.vue',
  );
  const filename = preview.locator('.file-viewer-filename');
  const directoryLabel = preview.locator('.file-viewer-directory');
  const close = preview.getByRole('button', { name: 'Close Preview' });
  await expect(directoryLabel).toHaveText('src/components');
  await expect(filename).toHaveCSS('font-weight', '400');
  expect(
    await filename.evaluate((element) => getComputedStyle(element).color),
  ).not.toBe(
    await directoryLabel.evaluate((element) => getComputedStyle(element).color),
  );
  const [headerBox, closeBox] = await Promise.all([
    preview.locator('.file-viewer-header').boundingBox(),
    close.boundingBox(),
  ]);
  expect(headerBox).not.toBeNull();
  expect(closeBox).not.toBeNull();
  expect((closeBox?.y ?? 0) + (closeBox?.height ?? 0) / 2).toBeCloseTo(
    (headerBox?.y ?? 0) + (headerBox?.height ?? 0) / 2,
  );
  await expect(preview).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(preview).toHaveCSS('background-color', 'rgb(0, 0, 0)');
  await page.emulateMedia({ colorScheme: 'light' });
  const previewText = preview.locator('.file-viewer-code');
  await expect(
    preview.getByText("export const component = 'TranscriptView';"),
  ).toBeVisible();
  await expect(previewText).toHaveCSS('-webkit-user-select', 'text');
  await expect(close).toBeFocused();
  await page.keyboard.press('Meta+A');
  expect(
    (await page.evaluate(() => window.getSelection()?.toString()))?.trimEnd(),
  ).toBe((await previewText.textContent())?.trimEnd());
  await expect(close).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(preview).toHaveCount(0);
  await expect(path).toBeFocused();

  await path.press('Enter');
  await expect(preview).toBeVisible();
  await preview.getByRole('button', { name: 'Close Preview' }).click();
  await expect(preview).toHaveCount(0);
  await expect(path).toBeFocused();

  await message.locator('[data-tau-path="src/components/"]').click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_OPENER_CALLS__))
    .toEqual(['plugin:opener|open_url', 'plugin:opener|open_path']);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      expect.objectContaining({
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          basePath: '/Users/someone/code/tau',
          projectPath: null,
          path: 'src/components/TranscriptView.vue',
        }),
      }),
      expect.objectContaining({ command: 'release_file_preview' }),
      expect.objectContaining({ command: 'prepare_file_preview' }),
      expect.objectContaining({ command: 'release_file_preview' }),
    ]);
});

test('links only filenames in line references and previews clean local paths', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const calls: unknown[] = [];
    const writes: string[] = [];
    Object.assign(window, { __TAU_PREVIEW_CALLS__: calls });
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string; path?: string },
      ): Promise<unknown> => {
        if (command === 'plugin:clipboard-manager|write_text') {
          writes.push(payload?.text ?? '');
          return Promise.resolve(null);
        }
        if (command === 'prepare_file_preview') {
          calls.push(payload);
          if (payload?.path !== 'src/components/ProjectSidebar.vue') {
            return Promise.reject(new Error('Unexpected preview path'));
          }
          const source = 'export const fixture = true;\n';
          return Promise.resolve({
            kind: 'ready',
            id: 'line-preview',
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
            filename: 'ProjectSidebar.vue',
            sourcePath:
              '/Users/someone/code/tau/src/components/ProjectSidebar.vue',
            byteLength: source.length,
          });
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(fixtureUrl);
  await waitForShowcaseRendering(page);
  const message = page.locator('[data-message-id="fixture-markdown-showcase"]');
  for (const [label, path, marker] of [
    ['Line', 'src/components/ProjectSidebar.vue', ':922'],
    ['Range', 'tests/playwright-config.test.ts', ':21–31'],
  ] as const) {
    const paragraph = message
      .locator('p')
      .filter({ hasText: `${label} reference:` })
      .first();
    const link = paragraph.locator('[data-tau-path]');
    await expect(link).toHaveText(path);
    await expect(paragraph).toContainText(`${path}${marker}`);
    expect(
      await link.evaluate((element) => element.nextSibling?.textContent),
    ).toBe(marker + (label === 'Line' ? '.' : ''));
    await expect(paragraph.locator('[data-tau-path]')).toHaveCount(1);
  }
  const line = message
    .locator('p')
    .filter({ hasText: 'Line reference:' })
    .first();
  const link = line.locator('[data-tau-path]');
  await link.scrollIntoViewIfNeeded();
  const markerBox = await link.evaluate((element) => {
    const node = element.nextSibling!;
    const range = document.createRange();
    range.setStart(node, 0);
    range.setEnd(node, 4);
    const { x, y, width, height } = range.getBoundingClientRect();
    return { x: x + width / 2, y: y + height / 2 };
  });
  await page.mouse.click(markerBox.x, markerBox.y);
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toEqual([]);
  await link.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Path', exact: true }).click();
  await link.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Full Path' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      'src/components/ProjectSidebar.vue',
      '/Users/someone/code/tau/src/components/ProjectSidebar.vue',
    ]);
  await link.click();
  const preview = page.getByRole('dialog', { name: 'ProjectSidebar.vue' });
  await expect(preview.getByText('export const fixture = true;')).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      expect.objectContaining({
        basePath: '/Users/someone/code/tau',
        projectPath: null,
        path: 'src/components/ProjectSidebar.vue',
      }),
    ]);
  await page.keyboard.press('Escape');
  await expect(link).toBeFocused();
  await link.press('Enter');
  await expect(preview).toBeVisible();
});

test('keeps unsupported previews in one terminal state', async ({ page }) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> => {
        if (command !== 'prepare_file_preview') return Promise.resolve(null);
        return Promise.resolve({
          kind: 'ready',
          id: 'unsupported-preview',
          assetPath: 'data:application/octet-stream,127.0.0.1%20localhost',
          filename: 'hosts',
          sourcePath: '/etc/hosts',
          byteLength: 19,
        });
      },
    };
  });
  await page.goto(fixtureUrl);
  await page.clock.install();

  await page
    .locator('[data-message-id="fixture-markdown-showcase"]')
    .locator('[data-tau-path="src/components/TranscriptView.vue"]')
    .click();
  const preview = page.getByRole('dialog', { name: 'hosts' });
  await expect(preview).toBeVisible();
  await expect(preview).not.toHaveAttribute('aria-busy');
  await expect(preview.getByText('Preview unavailable.')).toBeVisible();
  await expect(preview.locator('.file-viewer-object')).toHaveCount(0);

  await page.clock.fastForward(20_000);
  await expect(preview.getByText('Preview unavailable.')).toBeVisible();
  await expect(
    preview.getByText('Loading preview', { exact: true }),
  ).toHaveCount(0);
  await expect(
    preview.getByText('Couldn’t preview this file.', { exact: true }),
  ).toHaveCount(0);
});

test('scrolls large file previews in both directions', async ({ page }) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> => {
        if (command !== 'prepare_file_preview') return Promise.resolve(null);
        const source = `${Array.from(
          { length: 320 },
          (_, index) => `export const line${index} = ${index};`,
        ).join('\n')}\nexport const wide = '${'x'.repeat(2_000)}';\n`;
        return Promise.resolve({
          kind: 'ready',
          id: 'large-preview',
          assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
          filename: 'TranscriptView.vue',
          sourcePath:
            '/Users/someone/code/tau/src/components/TranscriptView.vue',
          byteLength: source.length,
        });
      },
    };
  });
  await page.goto(fixtureUrl);

  await page
    .locator('[data-message-id="fixture-markdown-showcase"]')
    .locator('[data-tau-path="src/components/TranscriptView.vue"]')
    .click();
  const content = page.locator('.file-viewer-content');
  await expect(content.locator('.shiki span').first()).toBeVisible();
  const overflow = await content.evaluate((element) => ({
    horizontal: element.scrollWidth > element.clientWidth,
    vertical: element.scrollHeight > element.clientHeight,
  }));
  expect(overflow).toEqual({ horizontal: true, vertical: true });
  await page.keyboard.press('PageDown');
  await page.keyboard.press('ArrowRight');
  await expect
    .poll(() =>
      content.evaluate((element) => ({
        left: element.scrollLeft,
        top: element.scrollTop,
      })),
    )
    .toMatchObject({ left: expect.any(Number), top: expect.any(Number) });
  expect(
    await content.evaluate((element) => element.scrollLeft),
  ).toBeGreaterThan(0);
  expect(
    await content.evaluate((element) => element.scrollTop),
  ).toBeGreaterThan(0);
  await page.keyboard.press('Home');
  await expect
    .poll(() => content.evaluate((element) => element.scrollTop))
    .toBe(0);
  await content.evaluate((element) => {
    element.scrollTo({ left: 400, top: 400 });
  });
  await expect
    .poll(() =>
      content.evaluate((element) => ({
        left: element.scrollLeft,
        top: element.scrollTop,
      })),
    )
    .toEqual({ left: 400, top: 400 });
});

test('previews remote range references and keeps explicit copy actions', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    const previewCalls: Array<{ command: string; payload?: unknown }> = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    Object.assign(window, { __TAU_PREVIEW_CALLS__: previewCalls });
    window.__TAURI_INTERNALS__ = {
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
          return Promise.resolve(null);
        }
        if (command === 'prepare_file_preview') {
          previewCalls.push({ command, payload });
          if ((payload as { path?: string })?.path !== 'src/ranged.ts') {
            return Promise.reject(new Error('Unexpected preview path'));
          }
          const source = 'export const remote = true;\n';
          return Promise.resolve({
            kind: 'ready',
            id: 'remote-preview',
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
            filename: 'ranged.ts',
            sourcePath: '/home/agent/rhinestone/src/ranged.ts',
            byteLength: source.length,
          });
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const line = page
    .locator('[data-message-id="fixture-remote-paths"] p')
    .filter({ hasText: 'Line reference:' });
  const path = line.getByRole('button', { name: 'Preview path src/ranged.ts' });
  await expect(path).toHaveText('src/ranged.ts');
  expect(
    await path.evaluate((element) => element.nextSibling?.textContent),
  ).toBe(':42-50');

  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Path', exact: true }).click();
  await path.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy Full Path' }).click();
  await path.focus();
  await path.press('Space');

  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['src/ranged.ts', '/home/agent/rhinestone/src/ranged.ts']);
  const preview = page.getByRole('dialog', { name: 'ranged.ts' });
  await expect(preview).toBeVisible();
  await expect(preview.locator('.file-viewer-directory')).toHaveText('src');
  await expect(preview.getByText('export const remote = true;')).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: 'ssh:fixture-project',
          basePath: '/home/agent/rhinestone',
          path: 'src/ranged.ts',
        }),
      },
    ]);
  await expect(page.locator('.path-feedback')).toHaveCount(0);
  await page.keyboard.press('Escape');
});

test('ignores duplicate remote activation while showing delayed loading', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const calls: string[] = [];
    let resolvePreview: ((value: unknown) => void) | undefined;
    Object.assign(window, {
      __TAU_PREVIEW_CALLS__: calls,
      __TAU_RESOLVE_PREVIEW__: (value: unknown) => resolvePreview?.(value),
    });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> => {
        if (command !== 'prepare_file_preview') return Promise.resolve(null);
        calls.push(command);
        return new Promise((resolve) => {
          resolvePreview = resolve;
        });
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);
  await waitForShowcaseRendering(page);

  const path = page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' });
  await path.click();
  await expect(page.locator('.path-feedback')).toHaveText('Opening…');
  await path.click();
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: string[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toEqual(['prepare_file_preview']);

  await page.evaluate(() => {
    const source = 'export const delayed = true;\n';
    (
      window as Window & {
        __TAU_RESOLVE_PREVIEW__?: (value: unknown) => void;
      }
    ).__TAU_RESOLVE_PREVIEW__?.({
      kind: 'ready',
      id: 'delayed-preview',
      assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
      filename: 'remote.ts',
      sourcePath: '/home/agent/rhinestone/src/remote.ts',
      byteLength: source.length,
    });
  });
  await expect(page.locator('.path-feedback')).toHaveCount(0);
  await expect(page.getByRole('dialog', { name: 'remote.ts' })).toBeVisible();
});

test('releases a preview prepared after its transcript unmounts', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const calls: Array<{ command: string; payload?: unknown }> = [];
    let resolvePreview: ((value: unknown) => void) | undefined;
    Object.assign(window, {
      __TAU_PREVIEW_CALLS__: calls,
      __TAU_RESOLVE_PREVIEW__: (value: unknown) => resolvePreview?.(value),
    });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string, payload?: unknown): Promise<unknown> => {
        calls.push({ command, payload });
        if (command !== 'prepare_file_preview') return Promise.resolve(null);
        return new Promise((resolve) => {
          resolvePreview = resolve;
        });
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  await page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' })
    .click();
  await page.evaluate(() =>
    (
      window as Window & {
        __TAU_TRANSCRIPT_FIXTURE__?: { switchSession: (key: string) => void };
      }
    ).__TAU_TRANSCRIPT_FIXTURE__?.switchSession('replacement'),
  );
  await page.evaluate(() => {
    const source = 'export const stale = true;\n';
    (
      window as Window & {
        __TAU_RESOLVE_PREVIEW__?: (value: unknown) => void;
      }
    ).__TAU_RESOLVE_PREVIEW__?.({
      kind: 'ready',
      id: 'stale-preview',
      assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
      filename: 'remote.ts',
      sourcePath: '/home/agent/rhinestone/src/remote.ts',
      byteLength: source.length,
    });
  });

  await expect(page.getByRole('dialog', { name: 'remote.ts' })).toHaveCount(0);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (
            window as Window & {
              __TAU_PREVIEW_CALLS__?: Array<{
                command: string;
                payload?: { id?: string };
              }>;
            }
          ).__TAU_PREVIEW_CALLS__,
      ),
    )
    .toContainEqual({
      command: 'release_file_preview',
      payload: expect.objectContaining({ id: 'stale-preview' }),
    });
});

test('shows generic remote preview failure copy', async ({ page }) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> =>
        command === 'prepare_file_preview'
          ? Promise.reject(new Error('connection lost'))
          : Promise.resolve(null),
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  await page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' })
    .click();
  await expect(page.locator('.path-feedback[role="status"]')).toHaveText(
    'Preview unavailable',
  );
});

test('shows remote preview busy copy', async ({ page }) => {
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string): Promise<unknown> =>
        command === 'prepare_file_preview'
          ? Promise.resolve({ kind: 'busy' })
          : Promise.resolve(null),
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  await page
    .locator('[data-message-id="fixture-remote-paths"]')
    .getByRole('button', { name: 'Preview path src/remote.ts' })
    .click();
  await expect(page.locator('.path-feedback[role="status"]')).toHaveText(
    'Can’t open right now',
  );
});

test('copies remote directories and previews regular files', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string; path?: string },
      ): Promise<unknown> => {
        if (
          command === 'plugin:clipboard-manager|write_text' &&
          payload?.text
        ) {
          writes.push(payload.text);
          return Promise.resolve(null);
        }
        if (command === 'prepare_file_preview') {
          if (payload?.path === 'src/remote.ts') {
            return Promise.resolve({ kind: 'directory' });
          }
          const source = '# Encoded file\n';
          return Promise.resolve({
            kind: 'ready',
            id: 'encoded-preview',
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
            filename: 'My File.md',
            sourcePath: '/home/agent/rhinestone/docs/My File.md',
            byteLength: source.length,
          });
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const message = page.locator('[data-message-id="fixture-remote-paths"]');
  await message
    .getByRole('button', { name: 'Preview path src/remote.ts' })
    .click();
  await message.getByRole('link', { name: 'Encoded file' }).click();

  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['src/remote.ts']);
  await expect(page.getByRole('dialog', { name: 'My File.md' })).toBeVisible();
});

test('decodes explicit file destinations and keeps invalid ones inert', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const previewCalls: Array<{ command: string; payload?: unknown }> = [];
    Object.assign(window, { __TAU_PREVIEW_CALLS__: previewCalls });
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (command: string, payload?: unknown): Promise<unknown> => {
        if (command === 'prepare_file_preview') {
          previewCalls.push({ command, payload });
          const source = '# Encoded file\n';
          return Promise.resolve({
            kind: 'ready',
            id: 'encoded-preview',
            assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(source)}`,
            filename: 'My File.md',
            sourcePath: '/home/agent/rhinestone/docs/My File.md',
            byteLength: source.length,
          });
        }
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  await page.getByRole('link', { name: 'Encoded file' }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
            .__TAU_PREVIEW_CALLS__,
      ),
    )
    .toEqual([
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: 'ssh:fixture-project',
          path: 'docs/My File.md',
        }),
      },
    ]);
  await expect(page.getByRole('dialog', { name: 'My File.md' })).toBeVisible();
  await page.keyboard.press('Escape');

  const url = page.url();
  await page.getByRole('link', { name: 'Invalid file' }).click();
  expect(page.url()).toBe(url);
  expect(
    await page.evaluate(
      () =>
        (window as Window & { __TAU_PREVIEW_CALLS__?: unknown[] })
          .__TAU_PREVIEW_CALLS__,
    ),
  ).toHaveLength(1);
});

test('copies a code block from a button the block reveals on hover', async ({
  page,
}) => {
  // The clipboard is a native command, which a browser test has to stand in
  // for; the writes it records are what the assertion reads.
  await page.addInitScript(() => {
    const writes: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
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
  await page.goto(fixtureUrl);
  await waitForShowcaseRendering(page);

  const message = page.locator('[data-message-id="fixture-markdown-showcase"]');
  const block = message.locator('.code-block').first();
  const copy = block.getByRole('button', { name: 'Copy Code' });
  const box = (await block.boundingBox()) ?? {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  };

  await expect(block.locator('pre')).toContainText(
    'export async function load',
  );
  await expect(copy).toHaveCSS('opacity', '0');

  await block.hover();
  await expect(copy).toHaveCSS('opacity', '0.45');

  // Pointing at the button itself is what takes it to full strength.
  await copy.hover();
  await expect(copy).toHaveCSS('opacity', '1');

  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      "export async function load(id: string): Promise<Session | null> {\n  // A comment, italic in both schemes\n  const session = await invoke<Session>('load_session', { id });\n  return session ?? null;\n}\n",
    ]);
  await expect(copy).toHaveAttribute('data-copied', 'true');

  // The acknowledgement belongs to the hovered block: leaving hides the button
  // even while it is still showing, and it is temporary in any case.
  await page.mouse.move(box.x + box.width / 2, box.y - 40);
  await expect(copy).toHaveCSS('opacity', '0');
  await expect(copy).not.toHaveAttribute('data-copied', 'true', {
    timeout: 3_000,
  });
});

test('leaves remote paths in fenced transcript markdown as code', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const writes: string[] = [];
    window.__TAU_CLIPBOARD_WRITES__ = writes;
    window.__TAURI_INTERNALS__ = {
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
        if (command === 'prepare_file_preview')
          return Promise.resolve({ kind: 'directory' });
        return Promise.resolve(null);
      },
    };
  });
  await page.goto(`${fixtureUrl}&remote=true`);

  const message = page.locator('[data-message-id="fixture-remote-paths"]');
  const workspace = message.getByRole('button', {
    name: 'Preview path /home/agent/rhinestone/workspace',
  });
  const block = message.locator('.code-block');
  const copy = block.getByRole('button', { name: 'Copy Code' });

  // Inline paths retain their remote-copy interaction, but full-width code is
  // literal content and must not become a row of independent path controls.
  await expect(workspace).toBeVisible();
  await expect(block.getByRole('button', { name: /Preview path/ })).toHaveCount(
    0,
  );
  await expect(block).toContainText('/home/agent/rhinestone/orchestrator');
  await expect(block).toContainText(
    '/home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md',
  );

  await workspace.focus();
  await workspace.press('Space');
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual(['/home/agent/rhinestone/workspace']);
  const feedback = page.locator('.path-feedback');
  await expect(feedback).toHaveText('Copied');
  await expect(feedback).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(feedback).not.toHaveCSS('box-shadow', 'none');

  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_CLIPBOARD_WRITES__))
    .toEqual([
      '/home/agent/rhinestone/workspace',
      'Repo /home/agent/rhinestone/orchestrator\nPlan /home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md\n/usage\n/ expanded\n</pre>\n',
    ]);
});
