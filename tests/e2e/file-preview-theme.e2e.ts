import type { Locator, Page } from '@playwright/test';

import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const triggerSelector =
  '[data-message-id="fixture-markdown-showcase"] [data-tau-path="src/components/TranscriptView.vue"]';
const transparentGif =
  'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
const silentWav =
  'data:audio/wav;base64,UklGRlYAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAATElTVBoAAABJTkZPSVNGVA0AAABMYXZmNjMuMS4xMDIAAGRhdGEQAAAAAAAAAAAAAAAAAAAAAAAAAA==';
const tinyMp4 =
  'data:video/mp4;base64,AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDEAAANebW9vdgAAAGxtdmhkAAAAAAAAAAAAAAAAAAAD6AAAAHgAAQAAAQAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAol0cmFrAAAAXHRraGQAAAADAAAAAAAAAAAAAAABAAAAAAAAAHgAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAAIAAAACAAAAAAAkZWR0cwAAABxlbHN0AAAAAAAAAAEAAAB4AAAEAAABAAAAAAIBbWRpYQAAACBtZGhkAAAAAAAAAAAAAAAAAAAyAAAABgBVxAAAAAAALWhkbHIAAAAAAAAAAHZpZGUAAAAAAAAAAAAAAABWaWRlb0hhbmRsZXIAAAABrG1pbmYAAAAUdm1oZAAAAAEAAAAAAAAAAAAAACRkaW5mAAAAHGRyZWYAAAAAAAAAAQAAAAx1cmwgAAAAAQAAAWxzdGJsAAAAwHN0c2QAAAAAAAAAAQAAALBhdmMxAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAIAAgBIAAAASAAAAAAAAAABFExhdmM2My4xLjEwMiBsaWJ4MjY0AAAAAAAAAAAAAAAAGP//AAAANmF2Y0MBZAAK/+EAGWdkAAqs2V+IiMBEAAADAAQAAAMAyDxIllgBAAZo6+PLIsD9+PgAAAAAEHBhc3AAAAABAAAAAQAAABRidHJ0AAAAAAAAvuIAAAAAAAAAGHN0dHMAAAAAAAAAAQAAAAMAAAIAAAAAFHN0c3MAAAAAAAAAAQAAAAEAAAAoY3R0cwAAAAAAAAADAAAAAQAABAAAAAABAAAGAAAAAAEAAAIAAAAAHHN0c2MAAAAAAAAAAQAAAAEAAAADAAAAAQAAACBzdHN6AAAAAAAAAAAAAAADAAACxQAAAAwAAAAMAAAAFHN0Y28AAAAAAAAAAQAAA44AAABhdWR0YQAAAFltZXRhAAAAAAAAACFoZGxyAAAAAAAAAABtZGlyYXBwbAAAAAAAAAAAAAAAACxpbHN0AAAAJKl0b28AAAAcZGF0YQAAAAEAAAAATGF2ZjYzLjEuMTAyAAAACGZyZWUAAALlbWRhdAAAAq4GBf//qtxF6b3m2Ui3lizYINkj7u94MjY0IC0gY29yZSAxNjUgcjMyMjIgYjM1NjA1YSAtIEguMjY0L01QRUctNCBBVkMgY29kZWMgLSBDb3B5bGVmdCAyMDAzLTIwMjUgLSBodHRwOi8vd3d3LnZpZGVvbGFuLm9yZy94MjY0Lmh0bWwgLSBvcHRpb25zOiBjYWJhYz0xIHJlZj0zIGRlYmxvY2s9MTowOjAgYW5hbHlzZT0weDM6MHgxMTMgbWU9aGV4IHN1Ym1lPTcgcHN5PTEgcHN5X3JkPTEuMDA6MC4wMCBtaXhlZF9yZWY9MSBtZV9yYW5nZT0xNiBjaHJvbWFfbWU9MSB0cmVsbGlzPTEgOHg4ZGN0PTEgY3FtPTAgZGVhZHpvbmU9MjEsMTEgZmFzdF9wc2tpcD0xIGNocm9tYV9xcF9vZmZzZXQ9LTIgdGhyZWFkcz0xIGxvb2thaGVhZF90aHJlYWRzPTEgc2xpY2VkX3RocmVhZHM9MCBucj0wIGRlY2ltYXRlPTEgaW50ZXJsYWNlZD0wIGJsdXJheV9jb21wYXQ9MCBjb25zdHJhaW5lZF9pbnRyYT0wIGJmcmFtZXM9MyBiX3B5cmFtaWQ9MiBiX2FkYXB0PTEgYl9iaWFzPTAgZGlyZWN0PTEgd2VpZ2h0Yj0xIG9wZW5fZ29wPTAgd2VpZ2h0cD0yIGtleWludD0yNTAga2V5aW50X21pbj0yNSBzY2VuZWN1dD00MCBpbnRyYV9yZWZyZXNoPTAgcmNfbG9va2FoZWFkPTQwIHJjPWNyZiBtYnRyZWU9MSBjcmY9MjMuMCBxY29tcD0wLjYwIHFwbWluPTAgcXBtYXg9NjkgcXBzdGVwPTQgaXBfcmF0aW89MS40MCBhcT0xOjEuMDAAgAAAAA9liIQAM//+9uy+BTYUyMEAAAAIQZoibEK//sAAAAAIAZ5BeQr/xIE=';
const tinyPdf =
  'data:application/pdf;base64,JVBERi0xLjQKJeLjz9MKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCAxIDFdIC9SZXNvdXJjZXMgPDwgPj4gL0NvbnRlbnRzIDQgMCBSID4+CmVuZG9iago0IDAgb2JqCjw8IC9MZW5ndGggMCA+PgpzdHJlYW0KCmVuZHN0cmVhbQplbmRvYmoKeHJlZgowIDUKMDAwMDAwMDAwMCA2NTUzNSBmIAowMDAwMDAwMDE1IDAwMDAwIG4gCjAwMDAwMDAwNjQgMDAwMDAgbiAKMDAwMDAwMDEyMSAwMDAwMCBuIAowMDAwMDAwMjIxIDAwMDAwIG4gCnRyYWlsZXIKPDwgL1NpemUgNSAvUm9vdCAxIDAgUiA+PgpzdGFydHhyZWYKMjcwCiUlRU9GCg==';

type Mode =
  | 'code'
  | 'markdown'
  | 'image'
  | 'audio'
  | 'video'
  | 'pdf'
  | 'unsupported'
  | 'empty'
  | 'truncated'
  | 'loading'
  | 'failure';

interface Preview {
  mode: Mode;
  filename: string;
  assetPath: string;
  source?: string;
  byteLength?: number;
}

const markdown = `---
title: Theme fixture
---
# Theme preview

Readable prose with **emphasis** and a [link](https://example.com).

\`\`\`ts
export const theme = true;
\`\`\`
`;

const previews: Preview[] = [
  {
    mode: 'code',
    filename: 'theme.ts',
    assetPath: 'data:text/plain,export%20const%20theme%20%3D%20true%3B',
    source: 'export const theme = true;',
  },
  {
    mode: 'markdown',
    filename: 'theme.md',
    assetPath: `data:text/plain,${encodeURIComponent(markdown)}`,
    source: markdown,
  },
  { mode: 'image', filename: 'transparent.gif', assetPath: transparentGif },
  { mode: 'audio', filename: 'silent.wav', assetPath: silentWav },
  { mode: 'video', filename: 'clip.mp4', assetPath: tinyMp4 },
  { mode: 'pdf', filename: 'document.pdf', assetPath: tinyPdf },
  {
    mode: 'unsupported',
    filename: 'archive.bin',
    assetPath: 'data:application/octet-stream,noop',
  },
  {
    mode: 'empty',
    filename: 'empty.md',
    assetPath: 'data:text/plain,',
    source: '',
    byteLength: 0,
  },
  {
    mode: 'truncated',
    filename: 'partial.txt',
    assetPath: 'data:text/plain,partial',
    source: 'partial',
    byteLength: 512 * 1024 + 1,
  },
  {
    mode: 'loading',
    filename: 'loading.txt',
    assetPath: 'https://preview.test/preview-theme-loading.txt',
    byteLength: 16,
  },
  {
    mode: 'failure',
    filename: 'failure.txt',
    assetPath: 'https://preview.test/preview-theme-failure.txt',
    byteLength: 10,
  },
];

async function openNext(page: Page, trigger: Locator): Promise<Locator> {
  await trigger.click();
  const preview = page.getByRole('dialog');
  await expect(preview).toBeVisible();
  return preview;
}

async function panelColor(page: Page): Promise<string> {
  return page.evaluate(() => {
    const probe = document.createElement('div');
    probe.style.background = 'var(--panel)';
    document.body.append(probe);
    const color = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return color;
  });
}

async function expectedHeaderGradient(header: Locator): Promise<string> {
  return header.evaluate((element) => {
    const { height, width } = element.getBoundingClientRect();
    const probe = document.createElement('div');
    probe.style.cssText = `position: fixed; width: ${width}px; height: ${height}px; background-image: linear-gradient(to bottom, color-mix(in srgb, var(--panel) 96%, transparent), color-mix(in srgb, var(--panel) 70%, transparent), transparent);`;
    document.body.append(probe);
    const gradient = getComputedStyle(probe).backgroundImage;
    probe.remove();
    return gradient;
  });
}

async function expectPanelChrome(
  page: Page,
  preview: Locator,
  truncated: boolean,
): Promise<void> {
  const panel = await panelColor(page);
  await expect(preview).toHaveCSS('background-color', panel);
  await expect(preview.locator('.file-viewer-header')).toHaveCSS(
    'background-image',
    await expectedHeaderGradient(preview.locator('.file-viewer-header')),
  );

  const badge = preview.locator('.file-viewer-limit');
  if (truncated) await expect(badge).toHaveCSS('background-color', panel);
  else await expect(badge).toHaveCount(0);
}

async function expectMediaMetadata(media: Locator): Promise<void> {
  const supported = await media.evaluate((element) =>
    (element as HTMLMediaElement).canPlayType(
      element instanceof HTMLVideoElement
        ? 'video/mp4; codecs="avc1.42E01E"'
        : 'audio/wav',
    ),
  );
  if (supported)
    await expect
      .poll(() =>
        media.evaluate((element) => (element as HTMLMediaElement).readyState),
      )
      .toBeGreaterThanOrEqual(1);
}

for (const initialTheme of ['light', 'dark'] as const) {
  test(`keeps every preview surface on the panel from ${initialTheme}`, async ({
    page,
  }, testInfo) => {
    let releaseLoading: (() => void) | undefined;
    if (initialTheme === 'dark')
      await page.setViewportSize({ width: 600, height: 720 });
    await page.emulateMedia({ colorScheme: initialTheme });
    await page.route(
      '**/preview-theme-loading.txt',
      (route): Promise<void> =>
        new Promise<void>((resolve): void => {
          releaseLoading = (): void => {
            void route.fulfill({ body: 'loading complete' }).then(resolve);
          };
        }),
    );
    await page.addInitScript((files: Preview[]) => {
      const fetch = window.fetch.bind(window);
      window.fetch = (input, init): Promise<Response> =>
        String(input).endsWith('/preview-theme-failure.txt')
          ? Promise.reject(new TypeError('Preview fixture fetch failed'))
          : fetch(input, init);
      let index = 0;
      Object.assign(window, {
        __TAURI_INTERNALS__: {
          transformCallback: (callback: unknown): unknown => callback,
          invoke: (command: string): Promise<unknown> => {
            if (command !== 'prepare_file_preview')
              return Promise.resolve(null);
            const file = files[index++]!;
            return Promise.resolve({
              kind: 'ready',
              id: `theme-${index}`,
              filename: file.filename,
              assetPath: file.assetPath,
              sourcePath: `/project/${file.filename}`,
              byteLength:
                file.byteLength ??
                new TextEncoder().encode(file.source ?? '').length,
            });
          },
        },
      });
    }, previews);
    await page.goto('/?fixture=long-transcript&preview=true');
    await waitForShowcaseRendering(page);
    const trigger = page.locator(triggerSelector);
    const inlineCode = page
      .locator('[data-message-id="fixture-markdown-showcase"] .code-block pre')
      .first();
    const inlineBackgrounds = new Map<string, string>();
    for (const theme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: theme });
      inlineBackgrounds.set(
        theme,
        await inlineCode.evaluate(
          (element) => getComputedStyle(element).backgroundColor,
        ),
      );
    }

    for (const expected of previews) {
      await page.emulateMedia({ colorScheme: initialTheme });
      const preview = await openNext(page, trigger);
      await expect(
        preview.getByText(expected.filename, { exact: true }),
      ).toBeVisible();

      if (expected.mode === 'code')
        await expect(preview.locator('.file-viewer-code')).toContainText(
          'theme',
        );
      if (expected.mode === 'markdown') {
        await expect(
          preview.getByRole('heading', { name: 'Theme preview' }),
        ).toBeVisible();
        await expect(preview.locator('.file-viewer-document')).toHaveCSS(
          'background-color',
          'rgba(0, 0, 0, 0)',
        );
      }
      if (expected.mode === 'image') {
        const image = preview.locator('img');
        await expect(image).toBeVisible();
        await expect
          .poll(() =>
            image.evaluate(
              (element) => (element as HTMLImageElement).naturalWidth,
            ),
          )
          .toBeGreaterThan(0);
      }
      if (expected.mode === 'audio') {
        const audio = preview.locator('audio');
        await expect(audio).toHaveAttribute('src', silentWav);
        await expectMediaMetadata(audio);
      }
      if (expected.mode === 'video') {
        const video = preview.locator('video');
        await expect(video).toHaveAttribute('src', tinyMp4);
        await expectMediaMetadata(video);
      }
      if (expected.mode === 'pdf') {
        const pdf = preview.locator('object[type="application/pdf"]');
        await expect(pdf).toHaveAttribute('data', tinyPdf);
        const geometry = await pdf.evaluate((element) => {
          const { height, width } = element.getBoundingClientRect();
          return { height, width };
        });
        expect(geometry.height).toBeGreaterThan(0);
        expect(geometry.width).toBeGreaterThan(0);
      }
      if (expected.mode === 'unsupported')
        await expect(preview.getByText('Preview unavailable.')).toBeVisible();
      if (expected.mode === 'empty')
        await expect(preview.getByText('Empty document')).toBeVisible();
      if (expected.mode === 'truncated')
        await expect(preview.getByText('Showing first 512 KiB')).toBeVisible();
      if (expected.mode === 'loading') {
        await expect(preview.getByRole('status')).toHaveText('Loading preview');
        await expectPanelChrome(page, preview, false);
        releaseLoading?.();
        await expect(preview.locator('.file-viewer-plain')).toContainText(
          'loading complete',
        );
      }
      if (expected.mode === 'failure')
        await expect(preview.getByRole('alert')).toHaveText(
          'Couldn’t preview this file.',
        );

      for (const theme of [
        initialTheme,
        initialTheme === 'light' ? 'dark' : 'light',
      ] as const) {
        await page.emulateMedia({ colorScheme: theme });
        await expectPanelChrome(page, preview, expected.mode === 'truncated');
        await expect(inlineCode).toHaveCSS(
          'background-color',
          inlineBackgrounds.get(theme)!,
        );
        if (theme === initialTheme)
          await preview.screenshot({
            path: `/tmp/tau-preview-${testInfo.project.name}-${expected.mode}-${theme}.png`,
          });
      }
      await page.keyboard.press('Escape');
      await expect(preview).toHaveCount(0);
      await expect(trigger).toBeVisible();
      await expect(inlineCode).toHaveCSS(
        'background-color',
        inlineBackgrounds.get(initialTheme === 'light' ? 'dark' : 'light')!,
      );
    }
  });
}
