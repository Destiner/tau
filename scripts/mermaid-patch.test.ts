import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const patch = readFileSync(
  new URL('../patches/beautiful-mermaid@1.1.3.patch', import.meta.url),
  'utf8',
);

describe('beautiful-mermaid dependency patch', () => {
  it('contains no trailing whitespace in the committed patch file', () => {
    const offendingLines = patch
      .split('\n')
      .flatMap((line, index) => (/[\t ]$/.test(line) ? [index + 1] : []));

    expect(offendingLines).toEqual([]);
  });
});
