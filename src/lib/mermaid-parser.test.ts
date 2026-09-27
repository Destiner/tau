import { parseMermaid } from 'beautiful-mermaid';
import { describe, expect, it } from 'vitest';

import { adaptMermaidSource } from './mermaid-source';

const codeLabels = `flowchart LR
  A["metadata.calls = request.tasks ?? []"] --> B["execute([bundle])"]
  B --> C["execute([parent, ...children])"]
  C --> D["submit {proofs: []}, sponsored: true"]`;

const forwardDefinitions = `graph TD
  D1 --> E
  F --> G
  E{"root authority"} --> F
  G["submit {proofs: []}, sponsored: true"] --> H
  E{"final authority"}
  H --> E`;

function graph(source: string): ReturnType<typeof parseMermaid> {
  const result = adaptMermaidSource(source);
  expect(result.kind).toBe('renderable');
  if (result.kind !== 'renderable')
    throw new Error('Unexpected unsafe fixture');
  return parseMermaid(result.source);
}

function nodes(source: string): [string, string, string][] {
  return [...graph(source).nodes.values()].map(({ id, label, shape }) => [
    id,
    label,
    shape,
  ]);
}

function edges(source: string): [string, string, string | undefined][] {
  return graph(source).edges.map(({ source, target, label }) => [
    source,
    target,
    label,
  ]);
}

/*
 * Patch rationale: https://github.com/lukilabs/beautiful-mermaid/issues/125.
 * Bun loads src/index.ts; ESM/Vite loads dist/index.js. Keep both patched
 * parsers equivalent; regenerate with `bun patch --commit` and check a frozen
 * install. Verify parseMermaid with both Bun and Node, these regressions plus
 * mermaid-source/mermaid tests, and transcript browser tests. Remove the patch
 * only when an upstream release passes all of them; this is a supported
 * flowchart subset, not the full Mermaid grammar.
 */
describe('patched beautiful-mermaid flowchart parser', () => {
  it('preserves ordered topology, code-like labels and forward definitions', () => {
    expect(nodes(codeLabels)).toEqual([
      ['A', 'metadata.calls = request.tasks ?? []', 'rectangle'],
      ['B', 'execute([bundle])', 'rectangle'],
      ['C', 'execute([parent, ...children])', 'rectangle'],
      ['D', 'submit {proofs: []}, sponsored: true', 'rectangle'],
    ]);
    expect(edges(codeLabels)).toEqual([
      ['A', 'B', undefined],
      ['B', 'C', undefined],
      ['C', 'D', undefined],
    ]);

    expect(nodes(forwardDefinitions)).toEqual([
      ['D1', 'D1', 'rectangle'],
      ['E', 'final authority', 'diamond'],
      ['F', 'F', 'rectangle'],
      ['G', 'submit {proofs: []}, sponsored: true', 'rectangle'],
      ['H', 'H', 'rectangle'],
    ]);
    expect(edges(forwardDefinitions)).toEqual([
      ['D1', 'E', undefined],
      ['F', 'G', undefined],
      ['E', 'F', undefined],
      ['G', 'H', undefined],
      ['H', 'E', undefined],
    ]);
  });

  it('consumes quoted labels across internal closers for every node shape', () => {
    for (const [shape, open, close] of [
      ['doublecircle', '(((', ')))'],
      ['stadium', '([', '])'],
      ['circle', '((', '))'],
      ['subroutine', '[[', ']]'],
      ['cylinder', '[(', ')]'],
      ['trapezoid', '[/', '\\]'],
      ['trapezoid-alt', '[\\', '/]'],
      ['asymmetric', '>', ']'],
      ['hexagon', '{{', '}}'],
      ['rectangle', '[', ']'],
      ['rounded', '(', ')'],
      ['diamond', '{', '}'],
    ]) {
      expect(
        nodes(
          `graph TD\n A${open}"inner ${close} --> {[]()} & café's &amp;"${close}`,
        ),
      ).toEqual([['A', `inner ${close} --> {[]()} & café's &amp;`, shape]]);
    }
  });

  it('keeps edge styles, classes, link styles, and ownership independent of definitions', () => {
    const source = `graph LR
  subgraph group [Group]
    A & B -->|yes| C
    C --> D
    D{"final"}:::hot
  end
  A -- maybe --> B
  class D quiet
  style D fill:#fff
  linkStyle 0 stroke:#123`;
    const parsed = graph(source);
    expect(parsed.subgraphs[0]?.nodeIds).toEqual(['A', 'B', 'C', 'D']);
    expect(parsed.classAssignments.get('D')).toBe('quiet');
    expect(parsed.nodeStyles.get('D')).toEqual({ fill: '#fff' });
    expect(parsed.linkStyles.get(0)).toEqual({ stroke: '#123' });
    expect(parsed.nodes.get('D')).toMatchObject({
      label: 'final',
      shape: 'diamond',
    });
    expect(edges(source)).toEqual([
      ['A', 'C', 'yes'],
      ['B', 'C', 'yes'],
      ['C', 'D', undefined],
      ['A', 'B', 'maybe'],
    ]);
  });

  it('rejects malformed statements and conflicting ownership rather than partial graphs', () => {
    for (const line of [
      'A["unterminated]',
      'A(((unfinished))',
      'A[unfinished',
      'A[valid] leftover',
      'A[valid]::: --> B',
      'A[valid]:::hot! --> B',
      'A -->',
      'A &',
      'A --> B &',
      'A --> B garbage',
      'A["quoted" garbage]',
      'A["ambiguous \\" quote"]',
    ]) {
      expect(() => graph(`graph TD\n ${line}`)).toThrow();
    }
    expect(() =>
      graph(`graph TD\n subgraph one\n A[x]\n end\n subgraph two\n A[y]\n end`),
    ).toThrow();
  });

  it('preserves nested subgraph ownership when updating a node', () => {
    const parsed = graph(`graph TD
  subgraph outer
    subgraph inner
      A --> B
      B{"named"}
    end
  end`);
    expect(parsed.subgraphs[0]?.children[0]?.nodeIds).toEqual(['A', 'B']);
    expect(parsed.nodes.get('B')).toMatchObject({
      label: 'named',
      shape: 'diamond',
    });
  });

  it('keeps quoted nested closers through the physical-newline adapter', () => {
    const source =
      'graph TD\r\n A["execute([bundle])\r\n\r\nnext<br>line\\nend"] --> B';
    expect(nodes(source)).toEqual([
      ['A', 'execute([bundle])\n\nnext\nline\nend', 'rectangle'],
      ['B', 'B', 'rectangle'],
    ]);
    expect(edges(source)).toEqual([['A', 'B', undefined]]);
  });
});

export { codeLabels, forwardDefinitions };
