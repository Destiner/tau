import { beforeAll, describe, expect, it, vi } from 'vitest';

import {
  accessibleDiagram,
  diagramCapabilities,
} from '../dev/diagram-fixtures';

import renderDiagram from './mermaid';

const FLOWCHART = 'graph TD\n  Prompt[Prompt] --> Answer[Answer]\n';
const SEQUENCE = 'sequenceDiagram\n  Tau->>Pi: prompt\n';

function svg(source: string): string {
  const result = renderDiagram(source, 'mermaid');
  expect(result).not.toBeNull();
  return result!;
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

  it.each(['A[unfinished', 'A -->'])(
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

  it('gives identical cached diagrams independent accessibility IDs', () => {
    const first = svg(accessibleDiagram);
    const second = svg(accessibleDiagram);
    expect(first).not.toBe(second);
    expect(textContent(first)).toBe(textContent(second));
    for (const id of ids(first)) expect(ids(second)).not.toContain(id);
  });
});
