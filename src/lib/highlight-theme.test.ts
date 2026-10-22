import { describe, expect, it } from 'vitest';

import matchedThemes from './highlight-theme';

describe('matched highlight themes', () => {
  it('provides complete light and dark semantic palettes', () => {
    for (const [appearance, theme] of Object.entries(matchedThemes)) {
      expect(theme.appearance).toBe(appearance);
      expect(theme.style['editor.background']).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.style['editor.foreground']).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.style.syntax).toEqual(
        expect.objectContaining({
          comment: expect.objectContaining({ font_style: 'italic' }),
          emphasis: expect.objectContaining({ font_style: 'italic' }),
          'emphasis.strong': expect.objectContaining({ font_weight: 700 }),
          string: expect.objectContaining({ color: expect.any(String) }),
          keyword: expect.objectContaining({ color: expect.any(String) }),
          tag: expect.objectContaining({ color: expect.any(String) }),
          'property.json_key': expect.objectContaining({
            color: expect.any(String),
          }),
          'punctuation.bracket.html': expect.objectContaining({
            color: expect.any(String),
          }),
          'tag.component.jsx': expect.objectContaining({
            color: expect.any(String),
          }),
        }),
      );
    }
  });
});
