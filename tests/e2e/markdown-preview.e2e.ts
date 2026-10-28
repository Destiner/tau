import type { Page } from '@playwright/test';

import {
  accessibleDiagram,
  diagramFence,
  unsafeDiagram,
} from '../../src/dev/diagram-fixtures';

import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&preview=true';
const transcriptPath = 'src/components/TranscriptView.vue';
const localRoot = '/Users/someone/code/tau';

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

test('isolates repeated diagrams and fullscreen accessibility across appearances', async ({
  page,
}) => {
  await openFixture(page, {
    [transcriptPath]: {
      filename: 'repeated.md',
      sourcePath: `${localRoot}/repeated.md`,
      source: [
        accessibleDiagram,
        accessibleDiagram,
        accessibleDiagram.replaceAll('Queue', 'Inbox'),
      ]
        .map(diagramFence)
        .join('\n\n'),
    },
  });
  await openDocument(page);
  // The fullscreen modal hides the underlying document from the accessibility tree.
  const document = page.getByRole('dialog', {
    name: 'repeated.md',
    includeHidden: true,
  });
  const diagrams = document.locator('.diagram > svg');
  await expect(diagrams).toHaveCount(3, { timeout: 20_000 });
  const assertReferences = async (): Promise<void> => {
    const issues = await page
      .locator('.diagram > svg, .viewer-drawing > svg')
      .evaluateAll((svgs) => {
        const ids = svgs.flatMap((svg) =>
          Array.from(svg.querySelectorAll('[id]'), (node) => node.id),
        );
        const issues = ids.filter((id, index) => ids.indexOf(id) !== index);
        for (const svg of svgs) {
          for (const attribute of ['aria-labelledby', 'aria-describedby']) {
            for (const id of (svg.getAttribute(attribute) ?? '')
              .split(/\s+/)
              .filter(Boolean)) {
              if (
                !Array.from(svg.querySelectorAll('[id]')).some(
                  (node) => node.id === id,
                )
              )
                issues.push(`${attribute}:${id}`);
            }
          }
        }
        return issues;
      });
    expect(issues).toEqual([]);
  };
  for (const svg of await diagrams.all()) {
    await expect(svg).toHaveAccessibleName('Delivery pipeline');
    await expect(svg).toHaveAccessibleDescription(
      'A request flows from queue to completion.',
    );
  }
  await assertReferences();
  const geometry = await diagrams.evaluateAll((svgs) =>
    svgs.map((svg) => svg.getAttribute('viewBox')),
  );
  const firstNode = diagrams
    .first()
    .locator('.pele-node')
    .first()
    .locator('rect');
  await page.emulateMedia({ colorScheme: 'light' });
  const light = await firstNode.evaluate((node) => getComputedStyle(node).fill);
  await document.locator('.diagram-expand').first().click();
  const viewer = page.getByRole('dialog', { name: 'Diagram', exact: true });
  await expect(viewer).toBeVisible();
  await assertReferences();
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect
    .poll(() => firstNode.evaluate((node) => getComputedStyle(node).fill))
    .not.toBe(light);
  const dark = await firstNode.evaluate((node) => getComputedStyle(node).fill);
  await expect(viewer.locator('.pele-node rect').first()).toHaveCSS(
    'fill',
    dark,
  );
  expect(
    await diagrams.evaluateAll((svgs) =>
      svgs.map((svg) => svg.getAttribute('viewBox')),
    ),
  ).toEqual(geometry);
  await page.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  await expect(document.locator('.diagram-expand').first()).toBeFocused();
  await assertReferences();
});

test('disables diagram navigation and relative or remote image loading', async ({
  page,
}) => {
  const requests: string[] = [];
  page.on('request', (request) => {
    if (/diagram\.invalid|diagram-relative\.png/.test(request.url()))
      requests.push(request.url());
  });
  await page.route('**/*diagram.invalid/**', (route) => route.abort());
  await openFixture(page, {
    [transcriptPath]: {
      filename: 'security.md',
      sourcePath: `${localRoot}/security.md`,
      source: diagramFence(unsafeDiagram),
    },
  });
  await openDocument(page);
  const document = page.getByRole('dialog', { name: 'security.md' });
  const diagram = document.locator('.diagram');
  await expect(diagram).toHaveCount(1, { timeout: 20_000 });
  await expect(diagram).toContainText('Safe label');
  await expect(
    diagram.locator('image, img, a[href], script, [onerror], [onclick]'),
  ).toHaveCount(0);
  await diagram.locator('.pele-node[data-id="A"]').click();
  expect(await page.evaluate(() => window.__TAU_MARKDOWN_URLS__)).toEqual([]);
  expect(
    await page.evaluate(() => window.__TAU_MARKDOWN_IMAGE_EXECUTED__),
  ).not.toBe(true);
  expect(requests).toEqual([]);
});

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

\`\`\`md
flowchart LR
  Markdown[Markdown fence] --> Source[Stays source]
\`\`\`

\`\`\`
flowchart LR
  Plain[Unlabelled fence] --> Source[Stays source]
\`\`\`

\`\`\`mermaid
flowchart LR
  A[Start] --> B[End]
  B --> C[/Ready to ship/]
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
    document.locator('.code-block[data-tau-lang="ts"] [data-tau-highlight]'),
  ).toContainText('const preview = true;');
  const markdownFence = document.locator('.code-block[data-tau-lang="md"]');
  const unlabelledFence = document
    .locator('.code-block:not([data-tau-lang])')
    .filter({ hasText: 'Unlabelled fence' });
  await expect(markdownFence).toContainText('Markdown fence');
  await expect(markdownFence.locator('.diagram')).toHaveCount(0);
  await expect(unlabelledFence).toHaveCount(1);
  await expect(unlabelledFence.locator('.diagram')).toHaveCount(0);
  const diagram = document.locator('.diagram > svg');
  await expect(diagram).toHaveCount(1, { timeout: 20_000 });
  await expect(diagram.locator('.pele-node[data-id="C"]')).toHaveClass(
    /pele-shape-lean-r/,
  );
  await expect(diagram.locator('.pele-node[data-id="C"] text')).toHaveText(
    'Ready to ship',
  );
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

  await page.setViewportSize({ width: 520, height: 700 });
  const tsCode = document.locator('.code-block[data-tau-lang="ts"]');
  await tsCode.getByRole('button', { name: 'Copy Code' }).click();
  await expect(tsCode.locator('.code-copy')).toHaveAttribute(
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
