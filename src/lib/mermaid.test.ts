import { type FlowchartModel, parse } from 'pele';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import {
  accessibleDiagram,
  diagramCapabilities,
} from '../dev/diagram-fixtures';

import renderDiagram from './mermaid';

const FLOWCHART = 'graph TD\n  Prompt[Prompt] --> Answer[Answer]\n';
const SEQUENCE = 'sequenceDiagram\n  Tau->>Pi: prompt\n';

const REPORTED_FLOWCHART = `flowchart TD
  Plan[Approved plan] --> Fixture[Synthetic fixture]
  Plan --> Review{Approved?}
  Fixture --> Regression[Browser regression]
  Review -->|No| Fixture
  Review -->|Yes| Regression
  Regression -- two<br/>lines --> Topology[Topology assertions]
  Topology -- count ≤ 7 --> Viewer[Diagram viewer]
  Viewer -- count > 7 --> Complete[/Ready to ship/]`;

const COMPACT_DOTTED_FLOWCHART = `flowchart TD
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

const MULTILINE_FLOWCHART = `flowchart LR
  E[Planner] --> F{Route exists
for both sides?}
  F -->|No| G[Decline]
  F -->|Yes| H[Price]
  E --> I{Delivery only
+ dynamic preview
+ no result?}
  I -->|Yes| J[Use fallback]`;

function graph(source: string): FlowchartModel {
  const model = parse(source);
  if (model.type !== 'flowchart') throw new Error('Expected a flowchart');
  return model;
}

function edges(source: string): string[][] {
  return graph(source).edges.map(({ start, end, text }) => [start, end, text]);
}

function svg(source: string): string {
  const result = renderDiagram(source, 'mermaid');
  expect(result).not.toBeNull();
  return result!;
}

function topology(source: string, nodes: number, edgeCount: number): string {
  const result = svg(source);
  expect([...result.matchAll(/class="pele-node(?:\s[^"]*)?"/g)]).toHaveLength(
    nodes,
  );
  expect([...result.matchAll(/class="pele-edge"/g)]).toHaveLength(edgeCount);
  return result;
}

function textContent(source: string): string {
  return source
    .replace(/<[^>]*>/g, ' ')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'")
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&')
    .replace(/\s+/g, ' ');
}

function ids(source: string): string[] {
  return [...source.matchAll(/ id="([^"]*)"/g)].map(([, id]) => id!);
}

describe('drawing a fenced diagram with Pele', () => {
  beforeAll(async () => {
    await vi.waitFor(
      () => expect(renderDiagram(FLOWCHART, 'mermaid')).not.toBeNull(),
      {
        timeout: 20_000,
      },
    );
  });

  it.each(diagramCapabilities)(
    'renders the $type family and its content',
    ({ source, labels }) => {
      const result = svg(source);
      expect(result).toContain('<svg');
      expect(result).toContain('viewBox');
      expect(result).toContain('class="pele ');
      for (const label of labels) expect(textContent(result)).toContain(label);
      expect(result).not.toContain('@import');
      expect(result).not.toContain('fonts.googleapis.com');
    },
  );

  it('preserves physical newlines without inventing nodes or losing branches', () => {
    const result = topology(MULTILINE_FLOWCHART, 6, 5);
    expect(edges(MULTILINE_FLOWCHART)).toEqual([
      ['E', 'F', ''],
      ['F', 'G', 'No'],
      ['F', 'H', 'Yes'],
      ['E', 'I', ''],
      ['I', 'J', 'Yes'],
    ]);
    expect(graph(MULTILINE_FLOWCHART).nodes.get('F')).toMatchObject({
      text: 'Route exists\nfor both sides?',
      type: 'diamond',
    });
    expect(graph(MULTILINE_FLOWCHART).nodes.get('I')).toMatchObject({
      text: 'Delivery only\n+ dynamic preview\n+ no result?',
      type: 'diamond',
    });
    expect([...result.matchAll(/pele-shape-diam/g)]).toHaveLength(2);
    for (const label of [
      'Route exists',
      'for both sides?',
      'Delivery only',
      '+ dynamic preview',
      '+ no result?',
    ]) {
      expect(result).toContain(label);
    }
  });

  it('keeps the complete reported topology, edge labels and final parallelogram', () => {
    const result = topology(REPORTED_FLOWCHART, 7, 8);
    expect(edges(REPORTED_FLOWCHART)).toEqual([
      ['Plan', 'Fixture', ''],
      ['Plan', 'Review', ''],
      ['Fixture', 'Regression', ''],
      ['Review', 'Fixture', 'No'],
      ['Review', 'Regression', 'Yes'],
      ['Regression', 'Topology', 'two<br/>lines'],
      ['Topology', 'Viewer', 'count ≤ 7'],
      ['Viewer', 'Complete', 'count > 7'],
    ]);
    expect(graph(REPORTED_FLOWCHART).nodes.get('Complete')).toMatchObject({
      text: 'Ready to ship',
      type: 'lean_right',
    });
    for (const label of [
      'two',
      'lines',
      'count ≤ 7',
      'count &gt; 7',
      'Ready to ship',
    ])
      expect(result).toContain(label);
    expect(result).toMatch(/pele-shape-lean-r[^>]*>\s*<polygon/);
  });

  it('keeps every node and directed edge in the compact dotted-arrow graph', () => {
    const result = topology(COMPACT_DOTTED_FLOWCHART, 11, 13);
    expect(
      edges(COMPACT_DOTTED_FLOWCHART).map(([from, to]) => [from, to]),
    ).toEqual([
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
    expect(graph(COMPACT_DOTTED_FLOWCHART).edges.at(-1)).toMatchObject({
      start: 'R',
      end: 'T',
      text: 'remaining-deadline fires',
      stroke: 'dotted',
      type: 'arrow_point',
    });
    for (const label of [
      'Start; α',
      'Accepted; ✓',
      'Retry?',
      'remaining-deadline fires',
    ])
      expect(result).toContain(label);
    expect(result).toContain('stroke-dasharray');
  });

  it.each([
    '-.retry-now.->',
    '-. retry-now.->',
    '-.retry-now .->',
    '-. retry-now .->',
  ])('renders compact dotted text with boundary padding: %s', (arrow) => {
    const source = `flowchart LR\n A[first\nsecond]:::hot ${arrow} B[/third\nfourth/]:::done`;
    const result = topology(source, 2, 1);
    expect(graph(source).edges[0]).toMatchObject({
      text: 'retry-now',
      stroke: 'dotted',
      type: 'arrow_point',
    });
    for (const label of ['first', 'second', 'third', 'fourth', 'retry-now'])
      expect(result).toContain(label);
  });

  it('preserves chained, bidirectional, unlabelled and thick edges', () => {
    for (const [line, expected] of [
      [
        'A -.retry; café & ready.-> B --> C',
        [
          ['A', 'B', 'retry; café & ready'],
          ['B', 'C', ''],
        ],
      ],
      ['A -.-> B', [['A', 'B', '']]],
      [
        'A -->|yes| B ==> C',
        [
          ['A', 'B', 'yes'],
          ['B', 'C', ''],
        ],
      ],
      ['A <-.backtrack.-> B', [['A', 'B', 'backtrack']]],
    ] as const) {
      const source = `flowchart LR\n ${line}`;
      topology(source, graph(source).nodes.size, expected.length);
      expect(edges(source)).toEqual(expected);
    }
    expect(graph('flowchart LR\n A <-.backtrack.-> B').edges[0]).toMatchObject({
      type: 'double_arrow_point',
      stroke: 'dotted',
    });
    expect(graph('flowchart LR\n A ==> B').edges[0]?.stroke).toBe('thick');
  });

  it('distinguishes slash shapes, multiline labels and later definitions', () => {
    const source = String.raw`graph LR
  A[/first
second/] --> B[\opposite\] --> C[/wide\] --> D[\narrow/]
  D --> E
  E[/named/]:::hot`;
    const result = topology(source, 5, 4);
    expect([...graph(source).nodes.values()].map(({ type }) => type)).toEqual([
      'lean_right',
      'lean_left',
      'trapezoid',
      'inv_trapezoid',
      'lean_right',
    ]);
    for (const label of [
      'first',
      'second',
      'opposite',
      'wide',
      'narrow',
      'named',
    ])
      expect(result).toContain(label);
    const polygons = [...result.matchAll(/<polygon points="([^"]+)"/g)].map(
      ([, points]) =>
        points!.split(' ').map((pair) => pair.split(',').map(Number)),
    );
    expect(polygons).toHaveLength(5);
    for (const corners of polygons.slice(0, 2)) {
      expect(corners).toHaveLength(4);
      expect(corners[0]![0]).not.toBe(corners[3]![0]);
      expect(corners[1]![0]).not.toBe(corners[2]![0]);
      expect(corners[0]![1]).toBe(corners[1]![1]);
      expect(corners[2]![1]).toBe(corners[3]![1]);
    }
    expect(Math.sign(polygons[0]![0]![0]! - polygons[0]![3]![0]!)).toBe(
      -Math.sign(polygons[1]![0]![0]! - polygons[1]![3]![0]!),
    );
    expect(svg('graph TD\n A[/"inner /] and \\]"/]')).toContain(
      'inner /] and \\]',
    );
  });

  it('preserves code-like quoted labels and forward definitions', () => {
    const source = `flowchart LR
  A["metadata.calls = request.tasks ?? []"] --> B["execute([bundle])"]
  B --> C["execute([parent, ...children])"]
  C --> D["submit {proofs: []}, sponsored: true"]`;
    const result = topology(source, 4, 3);
    for (const label of [
      'metadata.calls = request.tasks ?? []',
      'execute([bundle])',
      'execute([parent, ...children])',
      'submit {proofs: []}, sponsored: true',
    ])
      expect(textContent(result)).toContain(label);
    expect([...graph(source).nodes.values()].map(({ text }) => text)).toEqual([
      'metadata.calls = request.tasks ?? []',
      'execute([bundle])',
      'execute([parent, ...children])',
      'submit {proofs: []}, sponsored: true',
    ]);
    expect(edges(source)).toEqual([
      ['A', 'B', ''],
      ['B', 'C', ''],
      ['C', 'D', ''],
    ]);

    const forward = `graph TD
  D1 --> E
  F --> G
  E{"root authority"} --> F
  G["submit {proofs: []}, sponsored: true"] --> H
  E{"final authority"}
  H --> E`;
    const forwardSvg = topology(forward, 5, 5);
    expect(graph(forward).nodes.get('E')).toMatchObject({
      text: 'final authority',
      type: 'diamond',
    });
    expect(forwardSvg).toContain('final authority');
    expect(forwardSvg).not.toContain('root authority');
    expect(edges(forward)).toEqual([
      ['D1', 'E', ''],
      ['F', 'G', ''],
      ['E', 'F', ''],
      ['G', 'H', ''],
      ['H', 'E', ''],
    ]);
  });

  it.each([
    ['(((', ')))'],
    ['([', '])'],
    ['((', '))'],
    ['[[', ']]'],
    ['[(', ')]'],
    ['[/', '/]'],
    ['[\\', '\\]'],
    ['[/', '\\]'],
    ['[\\', '/]'],
    ['>', ']'],
    ['{{', '}}'],
    ['[', ']'],
    ['(', ')'],
    ['{', '}'],
  ])(
    'keeps quoted internal closers and physical newlines in %s … %s',
    (open, close) => {
      const label = `inner ${close} --> {[]()} & café's`;
      const source = `graph TD\n A${open}"${label}\nsecond"${close}`;
      const result = topology(source, 1, 0);
      expect(graph(source).nodes.get('A')?.text).toBe(`${label}\nsecond`);
      expect(result).toContain(
        label.replaceAll('&', '&amp;').replaceAll('>', '&gt;'),
      );
      expect(result).toContain('second');
      const unquoted = `graph TD\n A${open}first\nsecond${close}`;
      const unquotedSvg = topology(unquoted, 1, 0);
      expect(graph(unquoted).nodes.get('A')?.text).toBe('first\nsecond');
      expect(textContent(unquotedSvg)).toContain('first second');
    },
  );

  it('preserves CRLF, blank lines, existing breaks and multiple same-line labels', () => {
    const source =
      'graph TD\r\n A["execute([bundle])\r\n\r\nnext<br>line\\nend"] --> B';
    const result = topology(source, 2, 1);
    for (const label of ['execute([bundle])', 'next', 'line', 'end'])
      expect(result).toContain(label);
    const multiple = `graph LR
  A[first

customer's second line] --> B{third
fourth} & C(round
node)`;
    const multipleSvg = topology(multiple, 3, 2);
    for (const label of [
      'first',
      "customer's second line",
      'third',
      'fourth',
      'round',
      'node',
    ])
      expect(multipleSvg).toContain(label);
    topology(
      'graph TD\n A[one\\ntwo] --> B[one<br>two] --> C[one<br/>two] --> D[one<br />two]',
      4,
      3,
    );
    topology('graph TD\n A[one\ntwo]-.retry.->B{three\nfour}-.again.->C', 3, 2);
    expect(
      svg('graph TD\n A["literal -.retry.-> B\nstill a label"] --> C'),
    ).toContain('still a label');
  });

  it('allows comment-prefixed headers', () => {
    const result = topology(
      '\n%% generated\nflowchart TB\n A[first\nsecond] --> B{third\nfourth}',
      2,
      1,
    );
    for (const label of ['first', 'second', 'third', 'fourth'])
      expect(result).toContain(label);
  });

  it('preserves subgraph membership, nested ownership, classes and styles on redefinition', () => {
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
    const result = topology(source, 4, 4);
    expect(result).toContain('Group');
    expect(result).toContain('#fff');
    expect(result).toContain('#123');
    expect(graph(source).subgraphs[0]?.nodes.slice().sort()).toEqual([
      'A',
      'B',
      'C',
      'D',
    ]);
    expect(graph(source).nodes.get('D')).toMatchObject({
      text: 'final',
      type: 'diamond',
      classes: expect.arrayContaining(['quiet']),
    });
    expect(edges(source)).toEqual([
      ['A', 'C', 'yes'],
      ['B', 'C', 'yes'],
      ['C', 'D', ''],
      ['A', 'B', 'maybe'],
    ]);
    const nested =
      'graph TD\n subgraph outer\n subgraph inner\n A --> B\n B{"named"}\n end\n end';
    topology(nested, 2, 1);
    expect(
      graph(nested)
        .subgraphs.find(({ id }) => id === 'inner')
        ?.nodes.slice()
        .sort(),
    ).toEqual(['A', 'B']);
    expect(
      graph(nested).subgraphs.find(({ id }) => id === 'outer')?.nodes,
    ).toEqual(['inner']);
    expect(graph(nested).nodes.get('B')).toMatchObject({
      text: 'named',
      type: 'diamond',
    });
  });

  it('decodes entities as label content rather than rejecting quoted text', () => {
    expect(svg('graph TD\n A["fish &amp; chips"]')).toContain(
      'fish &amp; chips',
    );
    expect(textContent(svg('graph TD\n A["say &quot; hello"]'))).toContain(
      'say " hello',
    );
  });

  it('preserves accessible titles and descriptions', () => {
    const result = svg(accessibleDiagram);
    expect(result).toContain('Delivery pipeline');
    expect(result).toContain('A request flows from queue to completion.');
    expect(result).toContain('aria-labelledby=');
    expect(result).toContain('aria-describedby=');
  });

  it('scopes IDs and references independently for diagrams in one transcript', () => {
    const first = svg(accessibleDiagram);
    const second = svg(SEQUENCE);
    expect(ids(first).length).toBeGreaterThan(0);
    for (const id of ids(first)) expect(ids(second)).not.toContain(id);
    for (const result of [first, second]) {
      for (const [, reference] of result.matchAll(/url\(#([^)]*)\)/g))
        expect(ids(result)).toContain(reference);
      for (const [, references] of result.matchAll(
        /aria-(?:labelledby|describedby)="([^"]+)"/g,
      )) {
        for (const reference of references!.split(/\s+/))
          expect(ids(result)).toContain(reference);
      }
    }
  });

  it('falls back to source for unsupported fences, unknown diagrams and oversized input', () => {
    for (const fence of ['ts', 'md', ''])
      expect(renderDiagram(FLOWCHART, fence)).toBeNull();
    for (const source of [
      '',
      'not a diagram at all\n',
      `graph TD\n${'  A --> B\n'.repeat(1_000)}`,
    ]) {
      expect(renderDiagram(source, 'mermaid')).toBeNull();
    }
  });

  it.each([
    'A["unterminated}',
    'A(((unfinished))',
    'A[unfinished',
    'A[valid] leftover',
    'A[valid]::: --> B',
    'A -->',
    'A &',
    'A --> B &',
    'A --> B garbage',
    'A[/unfinished',
    'A[\\unfinished',
    'A[/valid/] garbage',
    'R -.label',
    'R -.label.->',
    'R -.label.-',
    'R -.label.-> T garbage',
    'R -. .-> T',
    'R -.label.-> T &',
    'A[unfinished\n B --> C',
    'A[unfinished\n B -.retry.-> C[done]',
    'A[unfinished\n B -. retry .-> C[done]',
    'A{"unfinished\nsecond}',
    'A[first\nsecond"still]',
    'A[first\nsecond] trailing text',
    'A["ambiguous \\" quote"]',
    'A[first\nsecond] :::hot --> B',
    'A[first\nsecond]::: --> B',
    'style[first\nsecond] --> class{third\nfourth}',
    'A[first\nsecond] -.retry.-> B[third\nfourth] trailing text',
  ])(
    'falls back rather than drawing an incomplete or malformed statement: %s',
    (line) => {
      expect(renderDiagram(`graph TD\n ${line}`, 'mermaid')).toBeNull();
    },
  );

  it('rejects edge expansion beyond the configured limit before layout', () => {
    const from = Array.from({ length: 80 }, (_, i) => `A${i}`).join(' & ');
    const to = Array.from({ length: 80 }, (_, i) => `B${i}`).join(' & ');
    const source = `flowchart LR\n ${from} --> ${to}`;
    expect(source.length).toBeLessThan(8_000);
    expect(renderDiagram(source, 'mermaid')).toBeNull();
  });

  it('keeps unresolved math and custom icons as visible label content without resolvers', () => {
    expect(textContent(svg('flowchart LR\n A["$$x^2$$"]'))).toContain(
      '$$x^2$$',
    );
    const icon = svg(
      'flowchart LR\n A@{ icon: "custom:missing", label: "Missing icon" }',
    );
    expect(textContent(icon)).toContain('Missing icon');
    expect(icon).toContain('data-icon="custom:missing"');
    expect(icon).not.toMatch(/<(?:image|foreignObject)\b/);
  });

  it('gives identical cached diagrams independent accessibility IDs', () => {
    const first = svg(accessibleDiagram);
    const second = svg(accessibleDiagram);
    expect(first).not.toBe(second);
    expect(textContent(first)).toBe(textContent(second));
    for (const id of ids(first)) expect(ids(second)).not.toContain(id);
  });
});
