import { describe, expect, it, vi } from 'vitest';

import {
  classifyFilePreview,
  escapePreviewHtml,
  fetchTextPreview,
  filePreviewAssetUrl,
  filePreviewDirectoryForPath,
  filePreviewDirectoryLabel,
  isBrowserPreviewUrl,
  normalizePreviewPath,
  resolveFilePreviewContextPath,
} from './file-preview';

describe('file preview classification', () => {
  it('maps text names and extensions to available Shiki languages', () => {
    expect(classifyFilePreview('service.TS')).toMatchObject({
      kind: 'text',
      language: 'typescript',
      presentation: 'source',
    });
    expect(classifyFilePreview('Dockerfile')).toMatchObject({
      kind: 'text',
      language: 'bash',
      presentation: 'source',
    });
    expect(classifyFilePreview('.gitignore')).toMatchObject({
      kind: 'text',
      presentation: 'source',
    });
  });

  it('presents Markdown documents as prose and MDX as source', () => {
    expect(classifyFilePreview('guide.MD')).toMatchObject({
      kind: 'text',
      presentation: 'markdown',
    });
    expect(classifyFilePreview('README')).toMatchObject({
      kind: 'text',
      presentation: 'markdown',
    });
    expect(classifyFilePreview('notes.MARKDOWN')).toMatchObject({
      kind: 'text',
      presentation: 'markdown',
    });
    expect(classifyFilePreview('component.MDX')).toMatchObject({
      kind: 'text',
      presentation: 'source',
    });
  });

  it('classifies safe browser media and leaves unknown files as objects', () => {
    expect(classifyFilePreview('diagram.svg')).toEqual({
      kind: 'image',
      mediaType: 'image/svg+xml',
    });
    expect(classifyFilePreview('manual.pdf').kind).toBe('pdf');
    expect(classifyFilePreview('demo.webm').kind).toBe('video');
    expect(classifyFilePreview('recording.flac').kind).toBe('audio');
    expect(classifyFilePreview('archive.zip')).toEqual({
      kind: 'object',
      mediaType: 'application/octet-stream',
    });
  });
});

describe('file preview directory labels', () => {
  it('normalizes paths lexically', () => {
    expect(normalizePreviewPath('/work/tau/./src/../docs')).toBe(
      '/work/tau/docs',
    );
    expect(normalizePreviewPath('src\\components\\..\\lib')).toBe('src/lib');
    expect(normalizePreviewPath('../../fixtures/../sample')).toBe(
      '../../sample',
    );
  });

  it('resolves document-relative paths without changing their encoding semantics', () => {
    expect(
      resolveFilePreviewContextPath('../images/My File.png', {
        sourcePath: '/work/tau/docs/guide/README.md',
        projectRoot: '/work/tau',
      }),
    ).toBe('/work/tau/docs/images/My File.png');
    expect(
      resolveFilePreviewContextPath('/etc/hosts', {
        sourcePath: '/work/tau/docs/README.md',
        projectRoot: '/work/tau',
      }),
    ).toBe('/etc/hosts');
    const remoteDocument = {
      sourcePath: '~/project/docs/README.md',
      projectRoot: '~/project',
      remoteIdentity: 'ssh:["agent@example.test","~/project"]',
    };
    expect(
      resolveFilePreviewContextPath('../images/diagram.svg', remoteDocument),
    ).toBe('~/project/images/diagram.svg');
    expect(
      resolveFilePreviewContextPath('~/notes/plan.md', remoteDocument),
    ).toBe('~/notes/plan.md');
  });

  it('uses relative labels inside a project and absolute labels outside it', () => {
    expect(
      filePreviewDirectoryForPath('/work/tau/src/ui/App.vue', '/work/tau'),
    ).toBe('src/ui');
    expect(filePreviewDirectoryForPath('/tmp/App.vue', '/work/tau')).toBe(
      '/tmp',
    );
    expect(filePreviewDirectoryForPath('/README.md', '/work/tau')).toBe('/');
    expect(filePreviewDirectoryLabel('/work/tau/src/ui', '/work/tau')).toBe(
      'src/ui',
    );
    expect(filePreviewDirectoryLabel('/work/tau/src/..', '/work/tau')).toBe(
      '.',
    );
    expect(filePreviewDirectoryLabel('/', '/')).toBe('.');
    expect(filePreviewDirectoryLabel('C:\\Work\\Tau', 'C:\\Work\\Tau')).toBe(
      '.',
    );
    expect(
      filePreviewDirectoryLabel('C:\\Work\\Tau\\src', 'c:\\work\\tau'),
    ).toBe('src');
    expect(filePreviewDirectoryLabel('/work/tau-other/src', '/work/tau')).toBe(
      '/work/tau-other/src',
    );
    expect(filePreviewDirectoryLabel('/tmp/../var/data', '/work/tau')).toBe(
      '/var/data',
    );
  });
});

describe('safe preview values', () => {
  it('escapes every HTML-significant character in plain text', () => {
    expect(escapePreviewHtml(`<img src=x onerror="alert('x')"> & text`)).toBe(
      '&lt;img src=x onerror=&quot;alert(&#39;x&#39;)&quot;&gt; &amp; text',
    );
  });

  it('renders an empty text snapshot without an unsatisfiable range request', async () => {
    const fetch = vi.spyOn(globalThis, 'fetch');

    await expect(fetchTextPreview('asset://empty.txt', 0)).resolves.toEqual({
      text: '',
      truncated: false,
    });
    expect(fetch).not.toHaveBeenCalled();

    fetch.mockRestore();
  });

  it('passes through only deterministic browser URL schemes', () => {
    const data = 'data:text/plain;charset=utf-8,hello';
    expect(isBrowserPreviewUrl(data)).toBe(true);
    expect(filePreviewAssetUrl(data)).toBe(data);
    expect(filePreviewAssetUrl('https://example.test/image.png')).toBe(
      'https://example.test/image.png',
    );
    expect(isBrowserPreviewUrl('javascript:alert(1)')).toBe(false);
    expect(isBrowserPreviewUrl('/tmp/example.txt')).toBe(false);
  });
});
