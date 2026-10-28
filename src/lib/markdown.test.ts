import { describe, expect, it } from 'vitest';

import {
  isWebUrl,
  linkFilePaths,
  parseFileReference,
  parseMarkdownFileDestination,
  protectLocalFileHrefs,
} from './markdown';

describe('web URLs', () => {
  it('accepts HTTP(S) and rejects other destinations', () => {
    expect(isWebUrl('https://example.com/docs')).toBe(true);
    expect(isWebUrl('http://localhost:3000')).toBe(true);
    expect(isWebUrl('mailto:hello@example.com')).toBe(false);
    expect(isWebUrl('/docs/getting-started')).toBe(false);
    expect(isWebUrl('not a URL')).toBe(false);
  });
});

describe('file references', () => {
  it('rejects prose, ambiguous roots and API route templates', () => {
    expect(parseFileReference('and/or')).toBeNull();
    expect(parseFileReference('24/7')).toBeNull();
    expect(parseFileReference('TypeScript/JavaScript')).toBeNull();
    expect(parseFileReference('2026/08/14')).toBeNull();
    for (const candidate of [
      '/pre',
      '/usage',
      '~/Documents',
      '/ expanded',
      '/Users/tim /notes.md',
    ]) {
      expect(parseFileReference(candidate)).toBeNull();
    }

    expect(parseFileReference('/README.md')?.path).toBe('/README.md');
    expect(parseFileReference('/tmp/')?.path).toBe('/tmp/');
    expect(parseFileReference('/etc/hosts')?.path).toBe('/etc/hosts');
    for (const candidate of [
      '/users/internal/orgs/:orgId/billing',
      '/billing/{events,suspend,resume}',
      '/users/[userId]/settings',
      '/files/*path',
    ]) {
      expect(parseFileReference(candidate)).toBeNull();
    }

    expect(parseFileReference('/Users/tim/orgs/billing')?.path).toBe(
      '/Users/tim/orgs/billing',
    );
    expect(parseFileReference('https://example.com/docs/guide.md')).toBeNull();
    expect(parseFileReference('//shared/report.md')).toBeNull();
    expect(parseFileReference('/')).toBeNull();
    expect(parseFileReference('~/')).toBeNull();
    expect(parseFileReference('README.md')).toBeNull();
  });
});

describe('explicit Markdown file destinations', () => {
  it('decodes explicit local destinations once, strips line suffixes and rejects unsafe URLs', () => {
    expect(parseMarkdownFileDestination('README')).toBe('README');
    expect(parseMarkdownFileDestination('docs/My%20File.md')).toBe(
      'docs/My File.md',
    );
    expect(parseMarkdownFileDestination('literal%2520name')).toBe(
      'literal%20name',
    );
    expect(parseMarkdownFileDestination('file:///tmp/My%20File.txt')).toBe(
      '/tmp/My File.txt',
    );
    expect(parseMarkdownFileDestination('file://localhost/tmp/a')).toBe(
      '/tmp/a',
    );
    expect(parseMarkdownFileDestination('file://server/tmp/a')).toBeNull();
    expect(parseMarkdownFileDestination('mailto:test@example.com')).toBeNull();
    expect(parseMarkdownFileDestination('%zz')).toBeNull();
    expect(parseMarkdownFileDestination('bad%0Aname')).toBeNull();
    expect(parseMarkdownFileDestination('#section')).toBeNull();
    expect(parseMarkdownFileDestination('src/App.vue:42')).toBe('src/App.vue');
    expect(parseMarkdownFileDestination('src/My%20App.vue:42:10')).toBe(
      'src/My App.vue',
    );
    for (const marker of [':21-31', ':21–31', ':21%E2%80%9331']) {
      expect(parseMarkdownFileDestination(`docs/My%20File.md${marker}`)).toBe(
        'docs/My File.md',
      );
      expect(
        parseMarkdownFileDestination(`file:///tmp/My%20File.md${marker}`),
      ).toBe('/tmp/My File.md');
    }
  });

  it('protects local file destinations and strips unsafe authorities', () => {
    const protectedLinks = protectLocalFileHrefs(
      '<a href="file:///tmp/a">local</a><a href="file://server/a">foreign</a>',
    );
    expect(protectedLinks.links).toEqual([
      expect.objectContaining({ href: 'file:///tmp/a' }),
    ]);
    expect(protectedLinks.html).toContain('https://tau.invalid/__file_');
    expect(protectedLinks.html).toContain('<a>foreign</a>');
  });
});

describe('linking file paths in markup', () => {
  it('links paths in prose and code while keeping line markers outside links', () => {
    expect(linkFilePaths('<p>Wrote src/App.vue:42, and stopped.</p>')).toBe(
      '<p>Wrote <a class="file-link" role="link" tabindex="0" data-tau-path="src/App.vue">src/App.vue</a>:42, and stopped.</p>',
    );
    const examples = [
      ['src/components/ProjectSidebar.vue', ':922'],
      ['tests/playwright-config.test.ts', ':21–31'],
      ['tests/playwright-config.test.ts', ':21-31'],
      ['./src/App.vue', ':42:10'],
      ['../tau/README.md', ':42'],
      ['/Users/tim/notes.md', ':42:10'],
      ['~/.config/pi/settings.json', ':21–31'],
    ];
    for (const [path, marker] of examples) {
      const anchor = `<a class="file-link" role="link" tabindex="0" data-tau-path="${path}">${path}</a>`;
      expect(linkFilePaths(`<p>See ${path}${marker}, then ${path}.</p>`)).toBe(
        `<p>See ${anchor}${marker}, then ${anchor}.</p>`,
      );
      expect(linkFilePaths(`<code>${path}${marker}</code>`, true)).toBe(
        `<code><a class="file-link" role="button" tabindex="0" aria-label="Preview path ${path}" data-tau-path="${path}">${path}</a>${marker}</code>`,
      );
    }
    expect(linkFilePaths('<p>src/my-file.ts:21-31!</p>')).toContain(
      'data-tau-path="src/my-file.ts">src/my-file.ts</a>:21-31!',
    );
  });

  it('links paths in code but not HTML attributes', () => {
    expect(linkFilePaths('<p><code>docs/pi.md</code></p>')).toBe(
      '<p><code><a class="file-link" role="link" tabindex="0" data-tau-path="docs/pi.md">docs/pi.md</a></code></p>',
    );
    expect(linkFilePaths('<p><code>../notes</code></p>')).toBe(
      '<p><code><a class="file-link" role="link" tabindex="0" data-tau-path="../notes">../notes</a></code></p>',
    );
    const image = '<p><img src="pictures/shot.png" alt="a/b.png"></p>';
    expect(linkFilePaths(image)).toBe(image);
  });

  it('skips existing links and fenced code, then resumes linking after a skipped element', () => {
    const link = '<p><a href="https://example.com/a/b.md">a/b.md</a></p>';
    expect(linkFilePaths(link)).toBe(link);

    const block = '<pre><code>/home/agent/rhinestone/sdk\n</code></pre>';
    expect(linkFilePaths(block)).toBe(block);
    expect(
      linkFilePaths('<p><a href="https://x.dev">x</a> holds src/App.vue</p>'),
    ).toBe(
      '<p><a href="https://x.dev">x</a> holds <a class="file-link" role="link" tabindex="0" data-tau-path="src/App.vue">src/App.vue</a></p>',
    );
  });

  it('leaves fenced paths unlinked when remote paths use buttons', () => {
    const block =
      '<pre><code>Repo /home/agent/rhinestone/orchestrator\nPlan /home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md\n</code></pre>';
    const html =
      `${block}<p>Workspace /home/agent/rhinestone/workspace ` +
      '<code>src/workspace.ts</code></p>';

    expect(linkFilePaths(html, true)).toBe(
      `${block}<p>Workspace <a class="file-link" role="button" tabindex="0" aria-label="Preview path /home/agent/rhinestone/workspace" data-tau-path="/home/agent/rhinestone/workspace">/home/agent/rhinestone/workspace</a> <code><a class="file-link" role="button" tabindex="0" aria-label="Preview path src/workspace.ts" data-tau-path="src/workspace.ts">src/workspace.ts</a></code></p>`,
    );
  });
});
