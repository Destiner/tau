import type { Page } from '@playwright/test';

import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&preview=true';
const transcriptPath = 'src/components/TranscriptView.vue';
const localRoot = '/Users/someone/code/tau';
const remoteRoot = '/home/agent/rhinestone';

interface Snapshot {
  filename: string;
  sourcePath: string;
  source: string;
  byteLength?: number;
}

interface PreviewCall {
  command: string;
  payload?: {
    path?: string;
    basePath?: string;
    projectPath?: string | null;
    id?: string;
  };
}

declare global {
  interface Window {
    __TAU_MARKDOWN_CALLS__?: PreviewCall[];
    __TAU_MARKDOWN_URLS__?: string[];
    __TAU_MARKDOWN_IMAGE_EXECUTED__?: boolean;
  }
}

async function openFixture(
  page: Page,
  snapshots: Record<string, Snapshot>,
  remote = false,
): Promise<void> {
  await page.addInitScript((files: Record<string, Snapshot>) => {
    const calls: PreviewCall[] = [];
    const urls: string[] = [];
    window.__TAU_MARKDOWN_CALLS__ = calls;
    window.__TAU_MARKDOWN_URLS__ = urls;
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback: unknown): unknown => callback,
      invoke: (
        command: string,
        payload?: { text?: string },
      ): Promise<unknown> => {
        const nativePayload = payload as PreviewCall['payload'];
        if (command === 'plugin:opener|open_url')
          urls.push((payload as { url?: string })?.url ?? '');
        if (
          command === 'prepare_file_preview' ||
          command === 'release_file_preview'
        ) {
          calls.push({ command, payload: nativePayload });
        }
        if (command !== 'prepare_file_preview') return Promise.resolve(null);
        const file = files[nativePayload?.path ?? ''];
        if (!file) return Promise.resolve({ kind: 'directory' });
        return Promise.resolve({
          kind: 'ready',
          id: `markdown-${calls.length}`,
          assetPath: `data:text/plain;charset=utf-8,${encodeURIComponent(file.source)}`,
          filename: file.filename,
          sourcePath: file.sourcePath,
          byteLength:
            file.byteLength ?? new TextEncoder().encode(file.source).length,
        });
      },
    };
  }, snapshots);
  await page.goto(remote ? `${fixtureUrl}&remote=true` : fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText(
    remote ? '2 messages' : '1 messages',
  );
  await waitForShowcaseRendering(page);
}

async function openDocument(page: Page, remote = false): Promise<void> {
  const path = remote
    ? page
        .locator('[data-message-id="fixture-remote-paths"]')
        .getByRole('button', { name: 'Preview path src/remote.ts' })
    : page
        .locator('[data-message-id="fixture-markdown-showcase"]')
        .locator(`[data-tau-path="${transcriptPath}"]`);
  await path.click();
}

test('reads a composite Markdown document, uses its controls, and follows a local link', async ({
  page,
}) => {
  const first = `${localRoot}/src/components/TranscriptView.vue`;
  const second = `${localRoot}/src/guide.md`;
  const source = `---
title: September release
tags:
  - desktop
  - release
authors:
  - name: Amina
    role: maintainer
unsafe: <img src=x onerror=alert(1)>
---
# Preview heading

## Second heading

# Release

# Notes

A paragraph of **prose**. [Next](../guide.md), [Guide](../guide.md) and [Site](https://example.com/guide).

- [x] Finished
- [ ] Pending

| Name | Count |
| :--- | ---: |
| Tau | 2 |

\`\`\`ts
const preview = true;
const value = 42;
\`\`\`

\`\`\`mermaid
flowchart LR
  A[Start] --> B[End]
\`\`\`

![Relative picture](./private.png)

<img src="./hostile.png" onerror="window.__TAU_MARKDOWN_IMAGE_EXECUTED__ = true">

<script>window.__TAU_MARKDOWN_IMAGE_EXECUTED__ = true</script>
`;
  await openFixture(page, {
    [transcriptPath]: { filename: 'README.md', sourcePath: first, source },
    [second]: {
      filename: 'guide.md',
      sourcePath: second,
      source: '# Guide opened\n',
    },
  });
  await openDocument(page);
  const dialog = page.getByRole('dialog', { name: 'README.md' });
  const document = dialog.locator('.file-viewer-document');
  const sheet = dialog.locator('.file-viewer-frontmatter');
  await expect(sheet).toBeVisible();
  await expect(sheet.locator('dt')).toHaveText([
    'title',
    'tags',
    'authors',
    'unsafe',
  ]);
  await expect(sheet.locator('dd')).toHaveText([
    'September release',
    'desktop, release',
    'name · Amina, role · maintainer',
    '<img src=x onerror=alert(1)>',
  ]);
  await expect(sheet.locator('img')).toHaveCount(0);
  const key = sheet.locator('dt').first();
  const textBounds = await key.evaluate((element) => {
    const range = globalThis.document.createRange();
    range.selectNodeContents(element);
    const { x, y, width, height } = range.getBoundingClientRect();
    return { x, y, width, height };
  });
  await page.mouse.move(textBounds.x + 1, textBounds.y + textBounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(
    textBounds.x + textBounds.width + 2,
    textBounds.y + textBounds.height / 2,
    { steps: 8 },
  );
  await page.mouse.up();
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe(
    'title',
  );
  await expect(dialog.getByText('Frontmatter', { exact: true })).toHaveCount(0);
  await expect(dialog.getByRole('heading', { name: 'Release' })).toBeVisible();
  await expect(dialog.getByRole('link', { name: 'Next' })).toBeVisible();
  await expect(dialog.locator('.markdown hr')).toHaveCount(0);

  await expect(
    document.getByRole('heading', { name: 'Preview heading', level: 1 }),
  ).toBeVisible();
  await expect(
    document.getByRole('heading', { name: 'Second heading', level: 2 }),
  ).toBeVisible();
  await expect(document.locator('input[type="checkbox"]')).toHaveCount(2);
  await expect(
    document.locator('input[type="checkbox"]').first(),
  ).toBeChecked();
  await expect(
    document.locator('input[type="checkbox"]').last(),
  ).not.toBeChecked();
  await expect(document.getByRole('cell', { name: 'Tau' })).toBeVisible();
  await expect(document.getByRole('cell', { name: '2' })).toBeVisible();
  await expect(
    document.locator('.code-block[data-tau-lang="ts"] pre.shiki'),
  ).toContainText('const preview = true;');
  await expect(document.locator('.diagram > svg')).toHaveCount(1, {
    timeout: 20_000,
  });
  await expect(dialog.locator('.file-viewer-code')).toHaveCount(0);
  await expect(document).toContainText('Relative picture');
  await expect(document.locator('img')).toHaveCount(0);
  await expect(document.locator('script')).toHaveCount(0);
  expect(
    await page.evaluate(() => window.__TAU_MARKDOWN_IMAGE_EXECUTED__),
  ).not.toBe(true);

  const expand = dialog.locator('.diagram-expand');
  await expect(expand).toHaveCount(1, { timeout: 20_000 });
  await expand.click();
  await expect(page.getByRole('dialog', { name: 'Diagram' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('dialog', { name: 'Diagram', exact: true }),
  ).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await dialog.getByRole('link', { name: 'Site' }).click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Copy URL' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menuitem', { name: 'Copy URL' })).toHaveCount(0);
  await expect(dialog).toBeVisible();

  await page.setViewportSize({ width: 1200, height: 700 });
  expect((await document.boundingBox())?.width).toBe(800);
  await page.setViewportSize({ width: 520, height: 700 });
  expect((await document.boundingBox())?.width).toBe(520);
  await dialog.getByRole('button', { name: 'Copy Code' }).click();
  await expect(dialog.locator('.code-copy')).toHaveAttribute(
    'data-copied',
    'true',
  );
  await dialog.getByRole('button', { name: 'Close Preview' }).focus();
  await page.keyboard.press('Meta+A');
  const selection = await page.evaluate(() =>
    window.getSelection()?.toString(),
  );
  expect(selection).toContain('Notes');
  expect(selection).toContain('const value = 42;');
  expect(selection).not.toContain('README.md');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(dialog).toHaveCSS('background-color', 'rgb(0, 0, 0)');

  await dialog.getByRole('heading', { name: 'Notes' }).click();
  await dialog.getByRole('link', { name: 'Site' }).click();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_MARKDOWN_URLS__))
    .toEqual(['https://example.com/guide']);
  await dialog.getByRole('link', { name: 'Guide' }).click();
  const replacement = page.getByRole('dialog', { name: 'guide.md' });
  await expect(
    replacement.getByRole('heading', { name: 'Guide opened' }),
  ).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_MARKDOWN_CALLS__))
    .toEqual([
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: null,
          basePath: localRoot,
          path: transcriptPath,
        }),
      },
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: null,
          basePath: `${localRoot}/src/components`,
          path: second,
        }),
      },
      {
        command: 'release_file_preview',
        payload: expect.objectContaining({ id: 'markdown-1' }),
      },
    ]);
  await page.keyboard.press('Escape');
  await expect(replacement).toHaveCount(0);
  await expect(
    page
      .locator('[data-message-id="fixture-markdown-showcase"]')
      .locator(`[data-tau-path="${transcriptPath}"]`),
  ).toBeFocused();
  await expect
    .poll(() => page.evaluate(() => window.__TAU_MARKDOWN_CALLS__?.at(-1)))
    .toEqual({
      command: 'release_file_preview',
      payload: expect.objectContaining({ id: 'markdown-2' }),
    });
});

test('resolves remote document links on the same project identity, without local opening', async ({
  page,
}) => {
  const first = `${remoteRoot}/src/remote.ts`;
  const second = `${remoteRoot}/docs/next.md`;
  await openFixture(
    page,
    {
      'src/remote.ts': {
        filename: 'remote.md',
        sourcePath: first,
        source: '[Next](../docs/next.md)\n',
      },
      [second]: {
        filename: 'next.md',
        sourcePath: second,
        source: '# Remote next\n',
      },
    },
    true,
  );
  await openDocument(page, true);
  await page
    .getByRole('dialog', { name: 'remote.md' })
    .getByRole('link', { name: 'Next' })
    .click();
  await expect(
    page
      .getByRole('dialog', { name: 'next.md' })
      .getByRole('heading', { name: 'Remote next' }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.__TAU_MARKDOWN_CALLS__?.filter(
          (call) => call.command === 'prepare_file_preview',
        ),
      ),
    )
    .toEqual([
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: 'ssh:fixture-project',
          basePath: remoteRoot,
          path: 'src/remote.ts',
        }),
      },
      {
        command: 'prepare_file_preview',
        payload: expect.objectContaining({
          projectPath: 'ssh:fixture-project',
          basePath: `${remoteRoot}/src`,
          path: second,
        }),
      },
    ]);
});

test('moves from a bounded snapshot through malformed frontmatter to an empty document', async ({
  page,
}) => {
  const draft = `${localRoot}/draft.md`;
  const empty = `${localRoot}/empty.md`;
  const oversized = `# First heading\n\n[Draft](./draft.md)\n\n${'a'.repeat(512 * 1024)}\n# Beyond limit\n`;
  await openFixture(page, {
    [transcriptPath]: {
      filename: 'long.md',
      sourcePath: `${localRoot}/long.md`,
      source: oversized,
    },
    [draft]: {
      filename: 'draft.md',
      sourcePath: draft,
      source: '---\ntitle: [invalid\n---\n# Draft\n\n[Empty](./empty.md)\n',
    },
    [empty]: {
      filename: 'empty.md',
      sourcePath: empty,
      source: '',
      byteLength: 0,
    },
  });
  await openDocument(page);
  const long = page.getByRole('dialog', { name: 'long.md' });
  await expect(
    long.getByRole('heading', { name: 'First heading' }),
  ).toBeVisible();
  await expect(long.getByRole('heading', { name: 'Beyond limit' })).toHaveCount(
    0,
  );
  await expect(long.getByText('Showing first 512 KiB')).toBeVisible();
  await long.getByRole('link', { name: 'Draft' }).click();
  const dialog = page.getByRole('dialog', { name: 'draft.md' });
  await expect(dialog.locator('.file-viewer-frontmatter')).toHaveCount(0);
  await expect(dialog.locator('.markdown')).toContainText('title: [invalid');
  await expect(
    dialog.getByRole('heading', { name: 'Draft', exact: true }),
  ).toBeVisible();
  await dialog.getByRole('link', { name: 'Empty' }).click();
  const blank = page.getByRole('dialog', { name: 'empty.md' });
  await expect(blank.getByText('Empty document')).toBeVisible();
  await expect(blank.locator('.markdown')).toHaveCount(0);
});

test('keeps MDX as highlighted source rather than running components', async ({
  page,
}) => {
  await openFixture(page, {
    [transcriptPath]: {
      filename: 'page.mdx',
      sourcePath: `${localRoot}/page.mdx`,
      source: '# MDX heading\n\n<Widget onClick={dangerous} />\n',
    },
  });
  await openDocument(page);
  const dialog = page.getByRole('dialog', { name: 'page.mdx' });
  await expect(dialog.locator('.file-viewer-code')).toContainText(
    '<Widget onClick={dangerous} />',
  );
  await expect(dialog.locator('.file-viewer-document')).toHaveCount(0);
  await expect(
    dialog.getByRole('heading', { name: 'MDX heading' }),
  ).toHaveCount(0);
  await expect(dialog.locator('widget')).toHaveCount(0);
});
