import { describe, expect, it } from 'vitest';

import { adaptMermaidSource } from './mermaid-source';

function adapted(source: string): string | null {
  const result = adaptMermaidSource(source);
  return result.kind === 'renderable' ? result.source : null;
}

describe('adapting Mermaid source for the renderer', () => {
  it('translates multiline node labels across all supported delimiters', () => {
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
    for (const [opener, closer] of [
      ['(((', ')))'],
      ['([', '])'],
      ['((', '))'],
      ['[[', ']]'],
      ['[(', ')]'],
      ['[/', '\\]'],
      ['[\\', '/]'],
      ['>', ']'],
      ['{{', '}}'],
      ['[', ']'],
      ['(', ')'],
      ['{', '}'],
    ]) {
      const source = `graph TD\n  A${opener}first\nsecond${closer}`;
      expect(adapted(source)).toBe(
        `graph TD\n  A${opener}first\\nsecond${closer}`,
      );
    }
  });

  it('supports quoted labels, internal closers, CRLF, blank lines and same-line edges', () => {
    const source =
      'graph LR\r\n  A["first\r\n  second\r\n\r\nfourth"] --> B[Done]\r\n';

    expect(adapted(source)).toBe(
      'graph LR\r\n  A["first\\n  second\\n\\nfourth"] --> B[Done]\r\n',
    );
    expect(adapted('graph TD\r\n  A["first ] and []\r\nsecond"] --> B')).toBe(
      'graph TD\r\n  A["first ] and []\\nsecond"] --> B',
    );
  });

  it('adapts multiple nodes and immediate class shorthand', () => {
    const source = `graph LR
  A[first

customer's second line] --> B{third
fourth} & C(round
node)`;

    expect(adapted(source)).toBe(`graph LR
  A[first\\n\\ncustomer's second line] --> B{third\\nfourth} & C(round\\nnode)`);
    const classified = `graph TD
  A[first
second]:::hot --> B`;
    expect(adapted(classified)).toBe(`graph TD
  A[first\\nsecond]:::hot --> B`);
  });

  it('preserves existing multiline forms, comments and non-flowchart statements', () => {
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

  it('recognizes keyword node IDs and headers following blank or comment lines', () => {
    const source = `graph TD
  style[first
second] --> class{third
fourth}`;

    expect(adapted(source)).toBe(`graph TD
  style[first\\nsecond] --> class{third\\nfourth}`);
    const prefixed = `
%% generated
flowchart TB
  A[one
two]`;
    expect(adapted(prefixed)).toBe(`
%% generated
flowchart TB
  A[one\\ntwo]`);
  });

  it('declines ambiguous or unterminated multiline labels', () => {
    for (const source of [
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
      `graph TD
  A(((first
second))`,
    ]) {
      expect(adaptMermaidSource(source)).toEqual({ kind: 'unsafe' });
    }
  });
});
