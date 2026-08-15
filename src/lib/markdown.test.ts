import { describe, expect, it } from 'vitest';

import {
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

  it('rejects prose that merely contains a separator', () => {
    expect(parseFileReference('and/or')).toBeNull();
    expect(parseFileReference('24/7')).toBeNull();
    expect(parseFileReference('TypeScript/JavaScript')).toBeNull();
    expect(parseFileReference('2026/08/14')).toBeNull();
  });

  it('rejects URLs and paths with nothing in them', () => {
    expect(parseFileReference('https://example.com/docs/guide.md')).toBeNull();
    expect(parseFileReference('//shared/report.md')).toBeNull();
    expect(parseFileReference('/')).toBeNull();
    expect(parseFileReference('~/')).toBeNull();
    expect(parseFileReference('README.md')).toBeNull();
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

  it('leaves links and code blocks as they are', () => {
    const link = '<p><a href="https://example.com/a/b.md">a/b.md</a></p>';
    expect(linkFilePaths(link)).toBe(link);

    const block = '<pre><code>cat src/App.vue\n</code></pre>';
    expect(linkFilePaths(block)).toBe(block);
  });

  it('resumes after the element it skipped', () => {
    expect(
      linkFilePaths('<p><a href="https://x.dev">x</a> holds src/App.vue</p>'),
    ).toBe(
      '<p><a href="https://x.dev">x</a> holds <a class="file-link" role="link" tabindex="0" data-tau-path="src/App.vue">src/App.vue</a></p>',
    );
  });

  it('does not read attributes as text', () => {
    const image = '<p><img src="pictures/shot.png" alt="a/b.png"></p>';
    expect(linkFilePaths(image)).toBe(image);
  });
});
