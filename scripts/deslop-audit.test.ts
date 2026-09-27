import { describe, expect, it } from 'vitest';

import {
  codeComments,
  markdownPaths,
  playwrightTests,
  resolveCoverage,
  rustTests,
} from './deslop-audit';

describe('deslop audit counters', () => {
  it('counts Unicode code points, not URLs or Rust raw strings, and nested Rust comments', () => {
    expect(codeComments('let a = "// not"; // 🦊\n/* ok */', '.ts')).toBe(
      5 + 7,
    );
    expect(
      codeComments(
        'const re = /https?:\\/\\/[^/]+/; // yes\nconst value = `// text ${1 /* inside */}`;',
        '.ts',
      ),
    ).toBe(6 + 12);
    expect(
      codeComments(
        'let a = r#"/* no */ // no"#; /* outer /* inner */ 🦊 */',
        '.rs',
      ),
    ).toBe(25);
    expect(
      codeComments(
        '<template><!-- 🦊 --></template><script setup lang="ts">const x = "//"; // yes</script><style>/* hi */</style>',
        '.vue',
      ),
    ).toBe(10 + 6 + 8);
  });

  it('deduplicates symlink-independent markdown paths and ignores build trees', () => {
    expect(
      markdownPaths(['docs/a.md', 'docs/a.md', 'dist/out.md'], ['new.md']),
    ).toEqual(['docs/a.md', 'new.md']);
  });

  it('counts only test records and deduplicates browser project discovery', () => {
    const report = {
      suites: [
        {
          title: 'a.e2e.ts',
          specs: [
            {
              file: 'a.e2e.ts',
              title: 'works',
              tests: [{ projectName: 'chromium' }, { projectName: 'webkit' }],
            },
          ],
        },
      ],
    };
    expect(playwrightTests(report)).toEqual({
      chromium: ['tests/e2e/a.e2e.ts > works'],
      webkit: ['tests/e2e/a.e2e.ts > works'],
    });
    expect(rustTests('abc: test\n1 tests, 0 benchmarks\nabc: test\n')).toEqual([
      'abc',
    ]);
  });

  it('follows transitive retained IDs, rejects uncovered originals and cycles', () => {
    const edges = [
      { from: ['a'], to: ['b'], reason: 'merged' },
      { from: ['b'], to: ['c'], reason: 'merged again' },
    ];
    expect(resolveCoverage(['a', 'z'], ['c'], edges)).toEqual(['z']);
    expect(resolveCoverage(['same', 'same'], ['same'], [])).toEqual(['same']);
    expect(() =>
      resolveCoverage(
        ['a'],
        [],
        [...edges, { from: ['c'], to: ['a'], reason: 'bad' }],
      ),
    ).toThrow('cycle');
  });
});
