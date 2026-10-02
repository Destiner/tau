import { parseMermaid } from 'beautiful-mermaid';
import { describe, expect, it } from 'vitest';

import { adaptMermaidSource } from './mermaid-source';

const codeLabels = `flowchart LR
  A["metadata.calls = request.tasks ?? []"] --> B["execute([bundle])"]
  B --> C["execute([parent, ...children])"]
  C --> D["submit {proofs: []}, sponsored: true"]`;

const reportedFlowchart = `flowchart TD
  Plan[Approved plan] --> Fixture[Synthetic fixture]
  Plan --> Review{Approved?}
  Fixture --> Regression[Browser regression]
  Review -->|No| Fixture
  Review -->|Yes| Regression
  Regression -- two<br/>lines --> Topology[Topology assertions]
  Topology -- count ≤ 7 --> Viewer[Diagram viewer]
  Viewer -- count > 7 --> Complete[/Ready to ship/]`;

const forwardDefinitions = `graph TD
  D1 --> E
  F --> G
  E{"root authority"} --> F
  G["submit {proofs: []}, sponsored: true"] --> H
  E{"final authority"}
  H --> E`;

const compactDottedTopology = `flowchart TD
  S[Start; α] --> R{Retry?}
  R --> Q[Queue]
  R --> P[Primary]
  R --> F[Fallback]
  F --> P2[Secondary] --> OK[Accepted; ✓]
  P & P2 --> W[Wait]
  E[External] --> W
  W --> Z[Retry timer]
  Z --> R
  W --> T[Timeout]
  R -.remaining-deadline fires.-> T`;

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
 * parsers and shape rendering equivalent, including compact dotted text
 * arrows and both parallelograms;
 * regenerate with `bun patch --commit` and check a frozen
 * install. Verify parseMermaid with both Bun and Node, these regressions plus
 * mermaid-source/mermaid tests, and transcript browser tests. Remove the patch
 * only when an upstream release passes all of them; this is a supported
 * flowchart subset, not the full Mermaid grammar.
 */
describe('patched beautiful-mermaid flowchart parser', () => {
  it('accepts dotted embedded text with every boundary-padding variant', () => {
    for (const arrow of [
      '-.retry-now.->',
      '-. retry-now.->',
      '-.retry-now .->',
      '-. retry-now .->',
    ]) {
      expect(graph(`flowchart LR\n R ${arrow} T`).edges).toEqual([
        expect.objectContaining({
          source: 'R',
          target: 'T',
          label: 'retry-now',
          style: 'dotted',
          hasArrowStart: false,
          hasArrowEnd: true,
        }),
      ]);
    }
    expect(edges('flowchart LR\n A -.retry; café & ready.-> B --> C')).toEqual([
      ['A', 'B', 'retry; café & ready'],
      ['B', 'C', undefined],
    ]);
    expect(edges('flowchart LR\n A -.-> B')).toEqual([['A', 'B', undefined]]);
    expect(edges('flowchart LR\n A -->|yes| B ==> C')).toEqual([
      ['A', 'B', 'yes'],
      ['B', 'C', undefined],
    ]);
    expect(graph('flowchart LR\n A <-.backtrack.-> B').edges[0]).toMatchObject({
      label: 'backtrack',
      style: 'dotted',
      hasArrowStart: true,
      hasArrowEnd: true,
    });
  });

  it('keeps every node and directed edge in a compact dotted-arrow topology', () => {
    const parsed = graph(compactDottedTopology);
    expect([...parsed.nodes.keys()]).toEqual([
      'S',
      'R',
      'Q',
      'P',
      'F',
      'P2',
      'OK',
      'W',
      'E',
      'Z',
      'T',
    ]);
    expect(parsed.nodes.get('R')).toMatchObject({
      label: 'Retry?',
      shape: 'diamond',
    });
    expect(parsed.nodes.get('S')?.label).toBe('Start; α');
    expect(parsed.nodes.get('OK')?.label).toBe('Accepted; ✓');
    expect(parsed.edges.map(({ source, target }) => [source, target])).toEqual([
      ['S', 'R'],
      ['R', 'Q'],
      ['R', 'P'],
      ['R', 'F'],
      ['F', 'P2'],
      ['P2', 'OK'],
      ['P', 'W'],
      ['P2', 'W'],
      ['E', 'W'],
      ['W', 'Z'],
      ['Z', 'R'],
      ['W', 'T'],
      ['R', 'T'],
    ]);
    expect(parsed.edges.at(-1)).toMatchObject({
      source: 'R',
      target: 'T',
      label: 'remaining-deadline fires',
      style: 'dotted',
      hasArrowStart: false,
      hasArrowEnd: true,
    });
  });

  it('rejects unfinished dotted text links and trailing garbage', () => {
    for (const line of [
      'R -.label',
      'R -.label.->',
      'R -.label.-',
      'R -.label.-> T garbage',
      'R -. .-> T',
      'R -.label.-> T &',
    ]) {
      expect(() => parseMermaid(`flowchart LR\n ${line}`)).toThrow();
    }
  });
  it('keeps the full reported topology, labels, and final parallelogram', () => {
    expect(nodes(`${reportedFlowchart}\n`)).toEqual([
      ['Plan', 'Approved plan', 'rectangle'],
      ['Fixture', 'Synthetic fixture', 'rectangle'],
      ['Review', 'Approved?', 'diamond'],
      ['Regression', 'Browser regression', 'rectangle'],
      ['Topology', 'Topology assertions', 'rectangle'],
      ['Viewer', 'Diagram viewer', 'rectangle'],
      ['Complete', 'Ready to ship', 'parallelogram'],
    ]);
    expect(edges(reportedFlowchart)).toEqual([
      ['Plan', 'Fixture', undefined],
      ['Plan', 'Review', undefined],
      ['Fixture', 'Regression', undefined],
      ['Review', 'Fixture', 'No'],
      ['Review', 'Regression', 'Yes'],
      ['Regression', 'Topology', 'two\nlines'],
      ['Topology', 'Viewer', 'count ≤ 7'],
      ['Viewer', 'Complete', 'count > 7'],
    ]);
  });

  it('distinguishes four slash shapes in a chain and later definitions', () => {
    const source = String.raw`graph LR
  A[/one/] --> B[\two\] --> C[/three\] --> D[\four/]
  D --> E
  E[/named/]:::hot`;
    expect(nodes(source)).toEqual([
      ['A', 'one', 'parallelogram'],
      ['B', 'two', 'parallelogram-alt'],
      ['C', 'three', 'trapezoid'],
      ['D', 'four', 'trapezoid-alt'],
      ['E', 'named', 'parallelogram'],
    ]);
    expect(edges(source).map(([from, to]) => [from, to])).toEqual([
      ['A', 'B'],
      ['B', 'C'],
      ['C', 'D'],
      ['D', 'E'],
    ]);
    expect(nodes('graph TD\n A[/"inner /] and \\]"/]')).toEqual([
      ['A', 'inner /] and \\]', 'parallelogram'],
    ]);
  });
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
      ['parallelogram', '[/', '/]'],
      ['parallelogram-alt', '[\\', '\\]'],
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
      'A[/unfinished',
      'A[\\unfinished',
      'A[/valid/] garbage',
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
