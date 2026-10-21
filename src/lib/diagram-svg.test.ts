import { describe, expect, it } from 'vitest';

import { scopeDiagramSvg } from './diagram-svg';

function ids(svg: string): string[] {
  return [...svg.matchAll(/\sid="([^"]*)"/g)].map((match) => match[1]!);
}

describe('scopeDiagramSvg', () => {
  it('gives identical occurrences and fullscreen copies independent identities', () => {
    const original =
      '<svg aria-labelledby="pele-title"><title id="pele-title">A diagram</title><path id="node" /></svg>';
    const inline = scopeDiagramSvg(original);
    const repeated = scopeDiagramSvg(original);
    const fullscreen = scopeDiagramSvg(inline);
    const allIds = [inline, repeated, fullscreen].flatMap(ids);

    expect(new Set(allIds).size).toBe(6);
    for (const svg of [inline, repeated, fullscreen]) {
      expect(svg).toContain(`aria-labelledby="${ids(svg)[0]}"`);
      expect(svg).toContain('>A diagram</title>');
    }
    expect(original).toContain('id="pele-title"');
  });

  it('rewrites forward references and every token in ARIA lists', () => {
    const svg = scopeDiagramSvg(
      '<svg aria-labelledby="title  subtitle" aria-describedby="desc\tmissing desc"><title id="title">Title</title><title id="subtitle">Subtitle</title><desc id="desc">Description</desc></svg>',
    );
    const [title, subtitle, desc] = ids(svg);

    expect(svg).toContain(`aria-labelledby="${title}  ${subtitle}"`);
    expect(svg).toContain(`aria-describedby="${desc}\tmissing ${desc}"`);
    expect(svg).toContain('>Title</title>');
    expect(svg).toContain('>Subtitle</title>');
    expect(svg).toContain('>Description</desc>');
  });

  it('rewrites fragment hrefs and local URL references in presentation attributes', () => {
    const svg = scopeDiagramSvg(
      `<svg><defs><path id="shape"/><marker id="arrow"/><clipPath id="clip"/></defs><use href="#shape" xlink:href='#shape'/><path marker-start="url(#arrow)" marker-mid="url( #arrow )" marker-end="url('#arrow')" clip-path='url("#clip")' style="fill:url(#shape);stroke:url(#shape)" mask="url(#shape)" filter="url(#shape)" fill="url(#shape)" stroke="url(#shape)"/></svg>`,
    );
    const [shape, arrow, clip] = ids(svg);

    expect(svg).toContain(`href="#${shape}" xlink:href='#${shape}'`);
    expect(svg).toContain(`marker-start="url(#${arrow})"`);
    expect(svg).toContain(`marker-mid="url(#${arrow})"`);
    expect(svg).toContain(`marker-end="url('#${arrow}')"`);
    expect(svg).toContain(`clip-path='url("#${clip}")'`);
    expect(svg).toContain(`style="fill:url(#${shape});stroke:url(#${shape})"`);
    for (const attribute of ['mask', 'filter', 'fill', 'stroke']) {
      expect(svg).toContain(`${attribute}="url(#${shape})"`);
    }
  });

  it('preserves label text, entities, comments, CDATA and non-reference attributes', () => {
    const label = 'id="node" #node url(#node) &amp; &lt;node&gt;';
    const text = `<text aria-label='${label}' data-label='${label}'>${label}</text>`;
    const comment = '<!-- <path id="not-an-id" fill="url(#node)"/> -->';
    const cdata = '<![CDATA[<path id="also-not-an-id"/> url(#node)]]>';
    const svg = scopeDiagramSvg(
      `<svg><path id="node"/>${text}${comment}${cdata}</svg>`,
    );

    expect(svg).toContain(text);
    expect(svg).toContain(comment);
    expect(svg).toContain(cdata);
    expect(svg).not.toContain('<path id="node"/>');
  });

  it('leaves unresolved references, colors and external resources unchanged', () => {
    const refs =
      '<use href="other.svg#node" xlink:href="https://example.com/#node"/><path fill="#abc" stroke="url(other.svg#node)" mask="url(#missing)"/><use href="#missing"/>';
    expect(scopeDiagramSvg(`<svg><path id="node"/>${refs}</svg>`)).toContain(
      refs,
    );
  });

  it('handles quoted greater-than signs, single quotes, and id-like attribute names', () => {
    const svg = scopeDiagramSvg(
      `<svg aria-labelledby = 'title'><title data-id="untouched" id = 'title'>A &gt; B</title><path aria-label="a > b" id="node"/><use href = '#node'/></svg>`,
    );
    const node = ids(svg)[0];
    const title = /<title data-id="untouched" id = '([^']+)'/.exec(svg)?.[1];

    expect(title).toBeDefined();
    expect(svg).toContain(`aria-labelledby = '${title}'`);
    expect(svg).toContain(`href = '#${node}'`);
    expect(svg).toContain('aria-label="a > b"');
    expect(svg).toContain('>A &gt; B</title>');
  });

  it('does not require IDs or a browser DOM', () => {
    const svg = '<svg><text>Plain diagram</text></svg>';
    expect(scopeDiagramSvg(svg)).toBe(svg);
  });
});
