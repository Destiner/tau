import { describe, expect, it } from 'vitest';

import {
  CODE_LANGUAGE_ATTRIBUTE,
  addCodeCopyButtons,
  addDiagramExpandButtons,
  isClosedFence,
  isPathOpenGesture,
  isWebUrl,
  linkFilePaths,
  removeEmptyTableHeaders,
  parseFileReference,
  parseMarkdownFileDestination,
  protectLocalFileHrefs,
  restoreProtectedLocalFileHrefs,
  resolveFilePath,
} from './markdown';

describe('web URLs', () => {
  it('accepts HTTP and HTTPS links', () => {
    expect(isWebUrl('https://example.com/docs')).toBe(true);
    expect(isWebUrl('http://localhost:3000')).toBe(true);
  });

  it('rejects non-web and relative links', () => {
    expect(isWebUrl('mailto:hello@example.com')).toBe(false);
    expect(isWebUrl('/docs/getting-started')).toBe(false);
    expect(isWebUrl('not a URL')).toBe(false);
  });
});

describe('file references', () => {
  it('reads rooted, relative, and home paths', () => {
    expect(parseFileReference('/Users/tim/notes.md')?.path).toBe(
      '/Users/tim/notes.md',
    );
    expect(parseFileReference('./src/App.vue')?.path).toBe('./src/App.vue');
    expect(parseFileReference('../tau/README.md')?.path).toBe(
      '../tau/README.md',
    );
    expect(parseFileReference('../notes')?.path).toBe('../notes');
    expect(parseFileReference('./build')?.path).toBe('./build');
    expect(parseFileReference('~/.config/pi/settings.json')?.path).toBe(
      '~/.config/pi/settings.json',
    );
    expect(parseFileReference('src/lib/markdown.ts')?.path).toBe(
      'src/lib/markdown.ts',
    );
  });

  it('keeps a line and column in the text and out of the path', () => {
    expect(parseFileReference('src/App.vue:42')).toEqual({
      text: 'src/App.vue:42',
      path: 'src/App.vue',
    });
    expect(parseFileReference('src/App.vue:42:10')).toEqual({
      text: 'src/App.vue:42:10',
      path: 'src/App.vue',
    });
  });

  it('leaves sentence punctuation outside the reference', () => {
    expect(parseFileReference('src/App.vue.')).toEqual({
      text: 'src/App.vue',
      path: 'src/App.vue',
    });
    expect(parseFileReference('src/App.vue:42:')).toEqual({
      text: 'src/App.vue:42',
      path: 'src/App.vue',
    });
  });

  it('accepts an unrooted directory only when it has a meaningful shape', () => {
    expect(parseFileReference('src/components/')?.path).toBe('src/components/');
    expect(parseFileReference('reports/148/')?.path).toBe('reports/148/');
    expect(parseFileReference('148/report.txt')?.path).toBe('148/report.txt');

    expect(parseFileReference('148/')).toBeNull();
    expect(parseFileReference('build/')).toBeNull();
  });

  it('keeps spaces and shell punctuation in a rooted path', () => {
    expect(
      parseFileReference('~/Documents/file with spaces & (parens).txt')?.path,
    ).toBe('~/Documents/file with spaces & (parens).txt');
  });

  it('rejects prose that merely contains a separator', () => {
    expect(parseFileReference('and/or')).toBeNull();
    expect(parseFileReference('24/7')).toBeNull();
    expect(parseFileReference('TypeScript/JavaScript')).toBeNull();
    expect(parseFileReference('2026/08/14')).toBeNull();
  });

  it('rejects ambiguous single-word roots and segment-edge spaces', () => {
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
  });

  it('rejects API route templates', () => {
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
  });

  it('rejects URLs and paths with nothing in them', () => {
    expect(parseFileReference('https://example.com/docs/guide.md')).toBeNull();
    expect(parseFileReference('//shared/report.md')).toBeNull();
    expect(parseFileReference('/')).toBeNull();
    expect(parseFileReference('~/')).toBeNull();
    expect(parseFileReference('README.md')).toBeNull();
  });
});

describe('path opening gesture', () => {
  it('uses ordinary primary click and Enter', () => {
    for (const platform of ['MacIntel', 'Win32', 'Linux x86_64']) {
      expect(isPathOpenGesture({ type: 'click', button: 0 }, platform)).toBe(
        true,
      );
    }
    expect(
      isPathOpenGesture({ type: 'keydown', key: 'Enter' }, 'MacIntel'),
    ).toBe(true);
  });

  it('leaves macOS Control-click for the context menu', () => {
    expect(
      isPathOpenGesture(
        { type: 'click', button: 0, ctrlKey: true },
        'MacIntel',
      ),
    ).toBe(false);
    expect(isPathOpenGesture({ type: 'click', button: 1 }, 'MacIntel')).toBe(
      false,
    );
  });
});

describe('explicit Markdown file destinations', () => {
  it('accepts bare names and decodes escaped path characters once', () => {
    expect(parseMarkdownFileDestination('README')).toBe('README');
    expect(parseMarkdownFileDestination('docs/My%20File.md')).toBe(
      'docs/My File.md',
    );
    expect(parseMarkdownFileDestination('literal%2520name')).toBe(
      'literal%20name',
    );
  });

  it('accepts local file URLs and rejects unsafe destinations', () => {
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
  });

  it('removes line suffixes after decoding', () => {
    expect(parseMarkdownFileDestination('src/App.vue:42')).toBe('src/App.vue');
    expect(parseMarkdownFileDestination('src/My%20App.vue:42:10')).toBe(
      'src/My App.vue',
    );
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

  it('restores dollar replacement patterns in file URLs literally', () => {
    expect(
      restoreProtectedLocalFileHrefs('<a href="placeholder">item</a>', [
        { placeholder: 'placeholder', href: "file:///tmp/$&-$'.txt" },
      ]),
    ).toBe('<a href="file:///tmp/$&amp;-$\'.txt">item</a>');
  });
});

describe('path resolution', () => {
  it('resolves relative paths against the session directory', () => {
    expect(resolveFilePath('/work/tau', 'src/App.vue')).toBe(
      '/work/tau/src/App.vue',
    );
    expect(resolveFilePath('/work/tau', './src/App.vue')).toBe(
      '/work/tau/src/App.vue',
    );
    expect(resolveFilePath('/work/tau', '../other/App.vue')).toBe(
      '/work/other/App.vue',
    );
    expect(resolveFilePath('/work/tau', '../notes')).toBe('/work/notes');
  });

  it('keeps rooted paths and expands the home directory', () => {
    expect(resolveFilePath('/work/tau', '/etc/hosts')).toBe('/etc/hosts');
    expect(resolveFilePath('/work/tau', '~/notes.md', '/Users/tim')).toBe(
      '/Users/tim/notes.md',
    );
    expect(resolveFilePath('/work/tau', '~/notes.md')).toBe('~/notes.md');
  });

  it('collapses redundant separators and steps', () => {
    expect(resolveFilePath('/work/tau/', 'docs//extensions.md')).toBe(
      '/work/tau/docs/extensions.md',
    );
    expect(resolveFilePath('/work/tau', '../../../etc/hosts')).toBe(
      '/etc/hosts',
    );
  });
});

describe('linking file paths in markup', () => {
  it('links a path and leaves the surrounding text alone', () => {
    expect(linkFilePaths('<p>Wrote src/App.vue:42, and stopped.</p>')).toBe(
      '<p>Wrote <a class="file-link" role="link" tabindex="0" data-tau-path="src/App.vue">src/App.vue:42</a>, and stopped.</p>',
    );
  });

  it('links paths written as code', () => {
    expect(linkFilePaths('<p><code>docs/extensions.md</code></p>')).toBe(
      '<p><code><a class="file-link" role="link" tabindex="0" data-tau-path="docs/extensions.md">docs/extensions.md</a></code></p>',
    );
    expect(linkFilePaths('<p><code>../notes</code></p>')).toBe(
      '<p><code><a class="file-link" role="link" tabindex="0" data-tau-path="../notes">../notes</a></code></p>',
    );
  });

  it('does not link slash commands or encoded markup as paths', () => {
    for (const html of [
      '<p><code>/usage</code></p>',
      '<p><code>/ expanded </code></p>',
      '<p><code>&lt;/pre&gt;</code></p>',
      '<p><code>&lt;/nested/tag&gt;</code></p>',
    ]) {
      expect(linkFilePaths(html)).toBe(html);
    }
  });

  it('does not link API route templates or ambiguous directory labels', () => {
    for (const html of [
      '<p><code>/users/internal/orgs/:orgId/billing</code></p>',
      '<p>Call <code>/billing/{events,suspend,resume}</code>.</p>',
      '<p>Next page: <code>148/</code>.</p>',
      '<p>Relative route: <code>:orgId/billing.json</code>.</p>',
    ]) {
      expect(linkFilePaths(html)).toBe(html);
    }
  });

  it('leaves existing links and fenced code blocks alone', () => {
    const link = '<p><a href="https://example.com/a/b.md">a/b.md</a></p>';
    expect(linkFilePaths(link)).toBe(link);

    const block = '<pre><code>/home/agent/rhinestone/sdk\n</code></pre>';
    expect(linkFilePaths(block)).toBe(block);
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

  it('resumes after the element it skipped', () => {
    expect(
      linkFilePaths('<p><a href="https://x.dev">x</a> holds src/App.vue</p>'),
    ).toBe(
      '<p><a href="https://x.dev">x</a> holds <a class="file-link" role="link" tabindex="0" data-tau-path="src/App.vue">src/App.vue</a></p>',
    );
  });

  it('detects absolute paths with hidden directories', () => {
    expect(
      linkFilePaths(
        '<p><code>/home/agent/.pi/workflows/implement/RHI-5900/implementation-plan.md</code></p>',
      ),
    ).toContain(
      'data-tau-path="/home/agent/.pi/workflows/implement/RHI-5900/implementation-plan.md"',
    );
  });

  it('links standalone rooted paths containing spaces and punctuation', () => {
    expect(
      linkFilePaths('<p>/Users/example/Library/Application Support</p>'),
    ).toBe(
      '<p><a class="file-link" role="link" tabindex="0" data-tau-path="/Users/example/Library/Application Support">/Users/example/Library/Application Support</a></p>',
    );

    const paths = [
      '/Users/example/Library/Application Support',
      '/Users/example/Video Projects',
      '/Users/example/Video Projects/Sample Recording 2026-01-01.project',
      '/Users/example/Library/Caches/com.example.browser/Webpage Previews',
      '~/Documents/file with spaces & (parens).txt',
    ];
    const encoded = paths.map((path) => path.replace(/&/g, '&amp;'));
    const html = linkFilePaths(
      encoded.map((path) => `<p>${path}</p>`).join(''),
    );

    for (const path of encoded) {
      expect(html).toContain(`data-tau-path="${path}"`);
    }
  });

  it('does not read attributes as text', () => {
    const image = '<p><img src="pictures/shot.png" alt="a/b.png"></p>';
    expect(linkFilePaths(image)).toBe(image);
  });
});

describe('table headers', () => {
  it('removes a header section only when every rendered header cell is empty', () => {
    expect(
      removeEmptyTableHeaders(
        '<table>\n<thead>\n<tr>\n<th align="left"></th>\n<th align="right"></th>\n</tr>\n</thead>\n<tbody><tr><td align="left">one</td><td align="right">two</td></tr></tbody>\n</table>',
      ),
    ).toBe(
      '<table>\n\n<tbody><tr><td align="left">one</td><td align="right">two</td></tr></tbody>\n</table>',
    );
  });

  it('keeps partially populated and non-empty headers unchanged', () => {
    const headers = [
      '<thead><tr><th></th><th>Kept</th></tr></thead>',
      '<thead><tr><th><img src="heading.png" alt=""></th></tr></thead>',
    ];

    for (const header of headers) {
      expect(removeEmptyTableHeaders(`<table>${header}</table>`)).toBe(
        `<table>${header}</table>`,
      );
    }
  });
});

describe('closed fences', () => {
  it('reads a block that reached its closing fence', () => {
    expect(isClosedFence('```mermaid\ngraph TD\n  A --> B\n```')).toBe(true);
    expect(isClosedFence('~~~\nplain\n~~~\n')).toBe(true);
    expect(isClosedFence('```mermaid\n```')).toBe(true);
  });

  it('reads a block that is still arriving', () => {
    // What every delta of a streamed answer ends in, and the case a diagram is
    // not drawn for: half of one is not a diagram.
    expect(isClosedFence('```mermaid\ngraph TD\n  A --> B\n')).toBe(false);
    expect(isClosedFence('```mermaid\ngraph TD\n  A --> ``')).toBe(false);
    expect(isClosedFence('```mermaid')).toBe(false);
  });
});

describe('code copy buttons', () => {
  it('wraps every fenced block, and only those', () => {
    const html = addCodeCopyButtons(
      '<p>prose with <code>inline</code></p><pre><code>const a = 1;\n</code></pre><p>more</p><pre>plain\n</pre>',
    );
    expect(html.match(/data-tau-copy/g)).toHaveLength(2);
    expect(html).toContain(
      '<div class="code-block"><pre><code>const a = 1;\n</code></pre><button type="button" class="code-copy" data-tau-copy aria-label="Copy Code">',
    );
    expect(html).toContain('<p>prose with <code>inline</code></p>');
  });

  it('keeps a block that writes its own closing tag whole', () => {
    const block = '<pre><code>echo "&lt;/pre&gt;"\n</code></pre>';
    const html = addCodeCopyButtons(block);
    expect(html).toContain(block);
    expect(html.match(/data-tau-copy/g)).toHaveLength(1);
  });

  it('carries the fenced language out to the wrapper the label is drawn on', () => {
    // The class is what marked writes, and what a highlighted block keeps.
    expect(
      addCodeCopyButtons(
        '<pre><code class="language-rust">fn main() {}\n</code></pre>',
      ),
    ).toContain(`<div class="code-block" ${CODE_LANGUAGE_ATTRIBUTE}="rust">`);
    expect(
      addCodeCopyButtons(
        '<pre class="shiki"><code class="shiki-code language-ts"><span>a</span></code></pre>',
      ),
    ).toContain(`<div class="code-block" ${CODE_LANGUAGE_ATTRIBUTE}="ts">`);

    // A fence that named no language has nothing to label.
    expect(addCodeCopyButtons('<pre><code>plain\n</code></pre>')).toContain(
      '<div class="code-block">',
    );
  });
});

describe('diagram expand buttons', () => {
  it('marks every drawn diagram, and only those', () => {
    const html = addDiagramExpandButtons(
      '<div class="diagram"><svg width="10" height="5"><g></g></svg></div>' +
        '<p>prose</p>' +
        '<div class="diagram">not a drawing</div>' +
        '<pre><code>graph TD\n</code></pre>',
    );

    expect(html.match(/data-tau-expand/g)).toHaveLength(1);
    // Inside the figure it expands, after the drawing itself.
    expect(html).toContain(
      '</svg><button type="button" class="diagram-expand" data-tau-expand aria-label="Expand Diagram">',
    );
    // A wrapper holding anything but a drawn svg is not a figure to expand.
    expect(html).toContain('<div class="diagram">not a drawing</div>');
    expect(html).toContain('<pre><code>graph TD\n</code></pre>');
  });

  it('gives each of several diagrams its own button', () => {
    const figure =
      '<div class="diagram"><svg width="2" height="1"></svg></div>';
    const html = addDiagramExpandButtons(`${figure}<p>between</p>${figure}`);

    expect(html.match(/data-tau-expand/g)).toHaveLength(2);
    expect(html).toContain('<p>between</p>');
  });
});
