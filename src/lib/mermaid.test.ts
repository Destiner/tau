import { describe, expect, it, vi } from 'vitest';

import renderDiagram from './mermaid';

const FLOWCHART = 'graph TD\n  Prompt[Prompt] --> Answer[Answer]\n';

const SEQUENCE = 'sequenceDiagram\n  Tau->>Pi: prompt\n';

const MULTILINE_FLOWCHART = `flowchart LR
  E[Planner] --> F{Route exists
for both sides?}
  F -->|No| G[Decline]
  F -->|Yes| H[Price]
  E --> I{Delivery only
+ dynamic preview
+ no result?}
  I -->|Yes| J[Use fallback]`;

/** The renderer is loaded asynchronously, so nothing is drawn on the first tick. */
async function ready(): Promise<void> {
  await vi.waitFor(
    () => {
      expect(renderDiagram(FLOWCHART, 'mermaid')).not.toBeNull();
    },
    { timeout: 20_000 },
  );
}

/** Every id the diagram gives an element of its own. */
function ids(svg: string): string[] {
  return [...svg.matchAll(/ id="([^"]*)"/g)].map(([, id]) => id ?? '');
}

describe('drawing a fenced diagram', () => {
  it('draws a diagram the parser can read', async () => {
    await ready();
    const svg = renderDiagram(SEQUENCE, 'mermaid');

    expect(svg).toContain('<svg');
    expect(svg).toContain('viewBox');
    expect(svg).toContain('prompt');
  });

  it('preserves physical newlines inside flowchart node labels', async () => {
    await ready();
    const svg = renderDiagram(MULTILINE_FLOWCHART, 'mermaid') ?? '';
    const nodes = [...svg.matchAll(/<g class="node"[^>]+>/g)].map(
      ([node]) => node,
    );
    const edges = [...svg.matchAll(/<polyline class="edge"[^>]*>/g)];

    expect(nodes).toHaveLength(6);
    expect(edges).toHaveLength(5);
    expect(
      edges.map(([edge]) => [
        /data-from="([^"]+)"/.exec(edge)?.[1],
        /data-to="([^"]+)"/.exec(edge)?.[1],
        /data-label="([^"]+)"/.exec(edge)?.[1],
      ]),
    ).toEqual([
      ['E', 'F', undefined],
      ['F', 'G', 'No'],
      ['F', 'H', 'Yes'],
      ['E', 'I', undefined],
      ['I', 'J', 'Yes'],
    ]);
    expect(
      nodes.filter((node) => node.includes('data-shape="diamond"')),
    ).toHaveLength(2);
    expect(nodes).toContainEqual(
      expect.stringContaining(
        'data-id="F" data-label="Route exists\nfor both sides?"',
      ),
    );
    expect(nodes).toContainEqual(
      expect.stringContaining(
        'data-id="I" data-label="Delivery only\n+ dynamic preview\n+ no result?"',
      ),
    );
    for (const label of [
      'Route exists',
      'for both sides?',
      'Delivery only',
      '+ dynamic preview',
      '+ no result?',
    ]) {
      expect(svg).toContain(label);
    }
    for (const label of ['F', 'I', 'for']) {
      expect(nodes.some((node) => node.includes(`data-label="${label}"`))).toBe(
        false,
      );
    }
  });

  it('renders complete quoted code labels and later explicit shapes', async () => {
    await ready();
    const svg =
      renderDiagram(
        `graph TD
  A["metadata.calls = request.tasks ?? []"] --> B
  B{"root authority"} --> C["submit {proofs: []}, sponsored: true"]`,
        'mermaid',
      ) ?? '';
    for (const [id, label, shape] of [
      ['A', 'metadata.calls = request.tasks ?? []', 'rectangle'],
      ['B', 'root authority', 'diamond'],
      ['C', 'submit {proofs: []}, sponsored: true', 'rectangle'],
    ]) {
      expect(svg).toContain(
        `data-id="${id}" data-label="${label}" data-shape="${shape}"`,
      );
      expect(svg).toContain(label);
    }
    expect(
      [...svg.matchAll(/<polyline class="edge"[^>]*>/g)].map(([edge]) => [
        /data-from="([^"]+)"/.exec(edge)?.[1],
        /data-to="([^"]+)"/.exec(edge)?.[1],
      ]),
    ).toEqual([
      ['A', 'B'],
      ['B', 'C'],
    ]);
  });

  it('decodes supported XML entities but rejects quote entities that break the label', async () => {
    await ready();
    const svg =
      renderDiagram('graph TD\n A["fish &amp; chips"]', 'mermaid') ?? '';
    expect(svg).toContain('data-label="fish &amp; chips"');
    expect(svg).toContain('fish &amp; chips');
    expect(
      renderDiagram('graph TD\n A["say &quot; hello"]', 'mermaid'),
    ).toBeNull();
  });

  it('does not render partially parsed flowcharts', async () => {
    await ready();
    for (const source of [
      'A["broken]',
      'A --> B garbage',
      'A -->',
      'A[ok]:::',
    ]) {
      expect(renderDiagram(`graph TD\n ${source}`, 'mermaid')).toBeNull();
    }
  });

  it('carries the app’s own colours rather than a palette of its own', async () => {
    await ready();
    const svg = renderDiagram(FLOWCHART, 'mermaid') ?? '';

    // References rather than values, so the scheme in use is the app's.
    expect(svg).toContain('--fg:var(--text)');
    expect(svg).toContain('--bg:var(--canvas)');
    // Every colour the library reads is given, including the three whose names
    // Tau's own tokens share: an underived one would inherit the app's value.
    for (const name of ['line', 'accent', 'muted', 'surface', 'border']) {
      expect(svg).toContain(`--${name}:var(--`);
    }
  });

  it('asks the network for nothing', async () => {
    await ready();
    // A class diagram asks for a second typeface for its members, so it is the
    // diagram with more than one request in it.
    const svg =
      renderDiagram('classDiagram\n  Session : +load()\n', 'mermaid') ?? '';

    expect(svg).not.toContain('@import');
    expect(svg).not.toContain('fonts.googleapis.com');
    // The typeface the app bundles, which is what the removed request was for.
    expect(svg).toContain("'Inter Variable'");
  });

  it('marks its ids as its own', async () => {
    await ready();
    const flowchart = renderDiagram(FLOWCHART, 'mermaid') ?? '';
    const sequence = renderDiagram(SEQUENCE, 'mermaid') ?? '';

    // Two diagrams in one transcript would otherwise share whichever arrow
    // head the document holds first, and a node named after part of the app
    // would collide with the app itself.
    expect(ids(flowchart).length).toBeGreaterThan(0);
    expect(ids(flowchart)).not.toContain('arrowhead');
    expect(ids(flowchart)).not.toContain('Prompt');
    for (const id of ids(flowchart)) expect(ids(sequence)).not.toContain(id);

    // Whatever is referenced by id is one the diagram gave itself.
    for (const [, reference] of flowchart.matchAll(/url\(#([^)]*)\)/g)) {
      expect(ids(flowchart)).toContain(reference);
    }
  });

  it('reports a fence it will not draw', async () => {
    await ready();

    expect(renderDiagram(FLOWCHART, 'ts')).toBeNull();
    expect(renderDiagram(FLOWCHART, '')).toBeNull();
    // Source the parser cannot read, which is what half of a streamed diagram
    // and a mislabelled fence both are.
    expect(renderDiagram('not a diagram at all\n', 'mermaid')).toBeNull();
    expect(renderDiagram('', 'mermaid')).toBeNull();
    // An incomplete multiline label would otherwise become a plausible but
    // partial graph, so it remains the source the reader was sent.
    expect(
      renderDiagram('graph TD\n  A[unfinished\n  B --> C', 'mermaid'),
    ).toBeNull();
    // Longer than a diagram, whatever else it is.
    expect(
      renderDiagram(`graph TD\n${'  A --> B\n'.repeat(1_000)}`, 'mermaid'),
    ).toBeNull();
  });

  it('holds a diagram it has already drawn', async () => {
    await ready();

    // The same diagram, ids and all: a re-mounted row and every delta of a
    // streamed message ask for the diagrams around them again.
    expect(renderDiagram(SEQUENCE, 'mermaid')).toBe(
      renderDiagram(SEQUENCE, 'mermaid'),
    );
  });
});
