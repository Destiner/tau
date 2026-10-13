import { expect, test } from 'bun:test';
import {
  isWebUrl,
  parseMarkdownFileDestination,
  resolveFilePath,
} from '../../src/lib/markdown';
import { resolveFilePreviewContextPath } from '../../src/lib/file-preview';

test('offline URL authority matches the Rust explicit-destination cases', () => {
  for (const written of [
    'https:example.com',
    'https:/example.com',
    'http:example.com',
    'HTTPS://Example.COM/a%20b?q=%5C#fragment',
    'https:///example.com',
    'https://[::1]:443/a',
    'https://example.com\\path',
    ' https://example.com ',
    'https://exa\tmple.com',
  ]) {
    expect(isWebUrl(written)).toBe(true);
  }
  for (const written of [
    'https:',
    'https://',
    'https://exa mple.com',
    'https://%zz',
    'https://[::1',
    'https://example.com:99999',
    'https://256.1.1.1',
    'https://user@',
    'mailto:person@example.com',
    'ftp://example.com',
    'javascript:alert(1)',
  ]) {
    expect(isWebUrl(written)).toBe(false);
    expect(parseMarkdownFileDestination(written)).toBeNull();
  }
  expect(new URL('https:example.com').href).toBe('https://example.com/');
  expect(new URL('https:/example.com').href).toBe('https://example.com/');
});

test('offline path authority preserves literal POSIX backslashes and decodes once', () => {
  const base = '/work/project\\root';
  for (const [written, expected] of [
    ['a\\b.txt', '/work/project\\root/a\\b.txt'],
    ['./a%5Cb.txt', '/work/project\\root/a%5Cb.txt'],
    ['/absolute\\root/a\\b.txt', '/absolute\\root/a\\b.txt'],
    ['src/../a%20b.txt', '/work/project\\root/a%20b.txt'],
  ]) {
    expect(resolveFilePath(base, written!)).toBe(expected!);
  }
  const decoded = parseMarkdownFileDestination('a%5Cb.txt');
  expect(decoded).toBe('a\\b.txt');
  expect(resolveFilePath(base, decoded!)).toBe('/work/project\\root/a\\b.txt');
  expect(parseMarkdownFileDestination('a%255Cb.txt')).toBe('a%5Cb.txt');
  expect(resolveFilePath(base, '~/a\\b%20.txt', '/home/user\\root')).toBe(
    '/home/user\\root/a\\b%20.txt',
  );
  expect(resolveFilePath(base, '~/a\\b%20.txt')).toBe('~/a\\b%20.txt');
  expect(
    resolveFilePreviewContextPath(decoded!, {
      sourcePath: '/work/project\\root/docs/README.md',
    }),
  ).toBe('/work/project/root/docs/a/b.txt');
});
