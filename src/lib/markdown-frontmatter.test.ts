import { describe, expect, it } from 'vitest';

import parsePreviewMarkdown from './markdown-frontmatter';

describe('file preview frontmatter', () => {
  it('separates a valid YAML mapping and preserves field order', () => {
    expect(
      parsePreviewMarkdown(
        '---\ntitle: September release\ndate: 2026-09-26\ntags:\n  - desktop\n  - release\n---\n# Release\n',
      ),
    ).toEqual({
      body: '# Release\n',
      fields: [
        { key: 'title', value: 'September release' },
        { key: 'date', value: '2026-09-26' },
        { key: 'tags', value: 'desktop, release' },
      ],
    });
  });

  it('shows nested and multiline values in the metadata sheet', () => {
    expect(
      parsePreviewMarkdown(
        '---\nsummary: |\n  First line\n  second line\nauthors:\n  - name: Amina\n    role: maintainer\n  - name: Jules\n    role: reviewer\npublished: false\n---\n',
      ),
    ).toEqual({
      body: '',
      fields: [
        { key: 'summary', value: 'First line second line' },
        {
          key: 'authors',
          value:
            'name · Amina, role · maintainer; name · Jules, role · reviewer',
        },
        { key: 'published', value: 'false' },
      ],
    });
  });

  it('strips empty frontmatter without adding an empty sheet', () => {
    expect(parsePreviewMarkdown('---\n---\n# Body')).toEqual({
      body: '# Body',
      fields: [],
    });
  });

  it('supports a BOM, CRLF and YAML document end marker', () => {
    expect(
      parsePreviewMarkdown('\uFEFF---\r\ntitle: Note\r\n...\r\n# Body'),
    ).toEqual({
      body: '# Body',
      fields: [{ key: 'title', value: 'Note' }],
    });
  });

  it.each([
    '# Heading\n---\ntitle: Not metadata\n---\n',
    '---\ntitle: Unclosed\n',
    '---\ntitle: [bad\n---\n# Body\n',
    '---\ntitle: First\ntitle: Second\n---\n# Body\n',
    '---\n- not a mapping\n---\n# Body\n',
    '---\nitem: &item [*item]\n---\n# Body\n',
  ])('leaves unsupported or incomplete input unchanged', (source) => {
    expect(parsePreviewMarkdown(source)).toEqual({ body: source, fields: [] });
  });
});
