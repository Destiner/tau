import { describe, expect, it } from 'vitest';

import {
  containsForbiddenContent,
  FORBIDDEN_CONTENT_CANARIES,
  matchedCanaries,
  sanitizeSourceLocation,
  truncateToLimit,
} from './privacy';

describe('forbidden content canaries', () => {
  it('detects a canary present in a larger string', () => {
    const promptCanary = FORBIDDEN_CONTENT_CANARIES.get('prompt');
    const haystack = `unrelated text ${promptCanary} more text`;
    expect(containsForbiddenContent(haystack)).toBe(true);
    expect(matchedCanaries(haystack)).toEqual(['prompt']);
  });

  it('does not trip on ordinary content', () => {
    expect(containsForbiddenContent('session started; rpc get_state ok')).toBe(
      false,
    );
    expect(matchedCanaries('session started')).toEqual([]);
  });
});

describe('sanitizeSourceLocation', () => {
  it('drops directory components, including a canary path', () => {
    const pathCanary = FORBIDDEN_CONTENT_CANARIES.get('projectPath') ?? '';
    const file = `${pathCanary}/src/index.ts`;
    const sanitized = sanitizeSourceLocation(file, 10, 4);
    expect(sanitized).toBe('index.ts:10:4');
    expect(containsForbiddenContent(sanitized)).toBe(false);
  });

  it('omits the column and fails closed for incomplete paths', () => {
    expect(sanitizeSourceLocation('lib.ts', 42)).toBe('lib.ts:42');
    expect(sanitizeSourceLocation('lib.ts')).toBe('lib.ts');
    expect(sanitizeSourceLocation('/private/project/src/')).toBe('src');
    expect(sanitizeSourceLocation('///')).toBe('unknown');
  });
});

describe('truncateToLimit', () => {
  it('leaves short values untouched', () => {
    expect(truncateToLimit('short', 128)).toBe('short');
  });

  it('uses UTF-8 bytes and never splits a codepoint', () => {
    const value = 'a'.repeat(127) + '😀';
    expect(truncateToLimit(value, 128)).toBe('a'.repeat(127));
    expect(truncateToLimit('é'.repeat(65), 128)).toBe('é'.repeat(64));
  });
});
