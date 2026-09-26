import { describe, expect, it } from 'vitest';

import { adaptMermaidSource } from './mermaid-source';

function adapted(source: string): string | null {
  const result = adaptMermaidSource(source);
  return result.kind === 'renderable' ? result.source : null;
}

describe('adapting Mermaid source for the renderer', () => {
  it('translates physical newlines inside complete flowchart node labels', () => {
    const source = `flowchart LR
  E[Planner] --> F{Route exists
for both sides?}
  F --> G[Decline]
  E --> I{Delivery only
+ dynamic preview
+ no result?}`;

    expect(adapted(source)).toBe(`flowchart LR
  E[Planner] --> F{Route exists\\nfor both sides?}
  F --> G[Decline]
  E --> I{Delivery only\\n+ dynamic preview\\n+ no result?}`);
  });

  it.each([
    ['double circle', '(((', ')))'],
    ['stadium', '([', '])'],
    ['circle', '((', '))'],
    ['subroutine', '[[', ']]'],
    ['cylinder', '[(', ')]'],
    ['trapezoid', '[/', '\\]'],
    ['inverse trapezoid', '[\\', '/]'],
    ['asymmetric', '>', ']'],
    ['hexagon', '{{', '}}'],
    ['rectangle', '[', ']'],
    ['rounded', '(', ')'],
    ['diamond', '{', '}'],
  ])('supports multiline %s labels', (_name, opener, closer) => {
    const source = `graph TD\n  A${opener}first\nsecond${closer}`;
    expect(adapted(source)).toBe(
      `graph TD\n  A${opener}first\\nsecond${closer}`,
    );
  });

  it('supports quoted labels, CRLF, blank lines, and same-line edges', () => {
    const source =
      'graph LR\r\n  A["first\r\n  second\r\n\r\nfourth"] --> B[Done]\r\n';

    expect(adapted(source)).toBe(
      'graph LR\r\n  A["first\\n  second\\n\\nfourth"] --> B[Done]\r\n',
    );
  });

  it('keeps interior closers in quoted multiline labels', () => {
    expect(adapted('graph TD\r\n  A["first ] and []\r\nsecond"] --> B')).toBe(
      'graph TD\r\n  A["first ] and []\\nsecond"] --> B',
    );
  });

  it('adapts multiple multiline nodes in one statement', () => {
    const source = `graph LR
  A[first

customer's second line] --> B{third
fourth} & C(round
node)`;

    expect(adapted(source)).toBe(`graph LR
  A[first\\n\\ncustomer's second line] --> B{third\\nfourth} & C(round\\nnode)`);
  });

  it('preserves valid immediate class shorthand after a multiline node', () => {
    const source = `graph TD
  A[first
second]:::hot --> B`;

    expect(adapted(source)).toBe(`graph TD
  A[first\\nsecond]:::hot --> B`);
  });

  it('leaves existing multiline forms and normal statements unchanged', () => {
    const source = String.raw`%% heading

flowchart TD
  A[one\ntwo] -->|an [edge] label| B[one<br>two]
  B --> C[one<br/>two] --> D[one<br />two]
  subgraph Group [A label]
    C --> D
  end
  classDef quiet fill:none
  class A quiet`;

    expect(adapted(source)).toBe(source);
  });

  it('ignores comments, non-node statements, and non-flowchart diagrams', () => {
    const flowchart = `graph TD
  %% unmatched [ " {
  subgraph Group [line
label]
  style A fill:[red
  A -->|edge [label
text]| B`;
    const state = `stateDiagram-v2
  state Composite {
    A --> B
  }`;

    expect(adapted(flowchart)).toBe(flowchart);
    expect(adapted(state)).toBe(state);
  });

  it('does not confuse statement keywords with node IDs', () => {
    const source = `graph TD
  style[first
second] --> class{third
fourth}`;

    expect(adapted(source)).toBe(`graph TD
  style[first\\nsecond] --> class{third\\nfourth}`);
  });

  it('allows leading blank and comment lines before a flowchart header', () => {
    const source = `
%% generated
flowchart TB
  A[one
two]`;
    expect(adapted(source)).toBe(`
%% generated
flowchart TB
  A[one\\ntwo]`);
  });

  it.each([
    `graph TD
  A[unfinished
  B --> C`,
    `graph TD
  A{"unfinished
second}`,
    `graph TD
  A[first
second"still]`,
    `graph TD
  A[first
second] trailing text`,
    `graph TD
  A[first
second] :::hot --> B`,
    `graph TD
  A[first
second]::: --> B`,
  ])('declines ambiguous or unterminated multiline labels', (source) => {
    expect(adaptMermaidSource(source)).toEqual({ kind: 'unsafe' });
  });

  it('does not reinterpret a more specific unterminated delimiter', () => {
    expect(
      adaptMermaidSource(`graph TD
  A(((first
second))`),
    ).toEqual({ kind: 'unsafe' });
  });
});
