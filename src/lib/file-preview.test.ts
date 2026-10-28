import { describe, expect, it, vi } from 'vitest';

import {
  escapePreviewHtml,
  fetchTextPreview,
  filePreviewAssetUrl,
  isBrowserPreviewUrl,
} from './file-preview';

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
