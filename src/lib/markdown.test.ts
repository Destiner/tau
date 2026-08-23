import { describe, expect, it } from 'vitest';

import {
  CODE_LANGUAGE_ATTRIBUTE,
  addCodeCopyButtons,
  isClosedFence,
  isPathOpenGesture,
  isWebUrl,
  linkFilePaths,
  parseFileReference,
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

  it('accepts a directory written with a trailing separator', () => {
    expect(parseFileReference('src/components/')?.path).toBe('src/components/');
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
      './build',
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

  it('rejects URLs and paths with nothing in them', () => {
    expect(parseFileReference('https://example.com/docs/guide.md')).toBeNull();
    expect(parseFileReference('//shared/report.md')).toBeNull();
    expect(parseFileReference('/')).toBeNull();
    expect(parseFileReference('~/')).toBeNull();
    expect(parseFileReference('README.md')).toBeNull();
  });
});

describe('path opening gesture', () => {
  it('uses Command-click on Apple platforms', () => {
    expect(
      isPathOpenGesture(
        { type: 'click', button: 0, metaKey: true },
        'MacIntel',
      ),
    ).toBe(true);
    expect(
      isPathOpenGesture(
        { type: 'click', button: 0, ctrlKey: true },
        'MacIntel',
      ),
    ).toBe(false);
  });

  it('uses Control-click on Windows and Linux', () => {
    for (const platform of ['Win32', 'Linux x86_64']) {
      expect(
        isPathOpenGesture(
          { type: 'click', button: 0, ctrlKey: true },
          platform,
        ),
      ).toBe(true);
      expect(
        isPathOpenGesture(
          { type: 'click', button: 0, metaKey: true },
          platform,
        ),
      ).toBe(false);
    }
  });

  it('ignores ordinary and non-primary clicks but keeps Enter accessible', () => {
    expect(isPathOpenGesture({ type: 'click', button: 0 }, 'MacIntel')).toBe(
      false,
    );
    expect(
      isPathOpenGesture(
        { type: 'click', button: 1, metaKey: true },
        'MacIntel',
      ),
    ).toBe(false);
    expect(
      isPathOpenGesture({ type: 'keydown', key: 'Enter' }, 'MacIntel'),
    ).toBe(true);
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

  it('links a path written as code', () => {
    expect(linkFilePaths('<p><code>docs/extensions.md</code></p>')).toBe(
      '<p><code><a class="file-link" role="link" tabindex="0" data-tau-path="docs/extensions.md">docs/extensions.md</a></code></p>',
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

  it('leaves existing links and fenced code blocks alone', () => {
    const link = '<p><a href="https://example.com/a/b.md">a/b.md</a></p>';
    expect(linkFilePaths(link)).toBe(link);

    const block = '<pre><code>/home/agent/rhinestone/sdk\n</code></pre>';
    expect(linkFilePaths(block)).toBe(block);
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
      linkFilePaths('<p>/Users/destiner/Library/Application Support</p>'),
    ).toBe(
      '<p><a class="file-link" role="link" tabindex="0" data-tau-path="/Users/destiner/Library/Application Support">/Users/destiner/Library/Application Support</a></p>',
    );

    const paths = [
      '/Users/destiner/Library/Application Support',
      '/Users/destiner/Screen Studio Projects',
      '/Users/destiner/Screen Studio Projects/My Recording 2026-08-17.screenstudio',
      '/Users/destiner/Library/Caches/com.apple.Safari/Webpage Previews',
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
