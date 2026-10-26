let scopes = 0;

const TAG =
  /<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\/?[A-Za-z][^"'>]*(?:(?:"[^"]*"|'[^']*')[^"'>]*)*>/g;
const ATTRIBUTE = /(\s)([^\s=/>]+)(\s*=\s*)(["'])([\s\S]*?)\4/g;
const URL_REFERENCE = /url\(\s*(["']?)#([^\s"')]+)\1\s*\)/g;
const URL_ATTRIBUTES = new Set([
  'style',
  'fill',
  'stroke',
  'clip-path',
  'mask',
  'filter',
  'marker-start',
  'marker-mid',
  'marker-end',
]);

/** Isolate renderer output and fullscreen copies without touching label text. */
// eslint-disable-next-line import-x/prefer-default-export -- Named shared renderer/UI boundary.
export function scopeDiagramSvg(svg: string): string {
  const scope = `diagram-${++scopes}`;
  const ids = new Map<string, string>();

  function mapAttributes(
    tag: string,
    rewrite: (name: string, value: string) => string,
  ): string {
    if (tag.startsWith('<!')) return tag;
    return tag.replace(
      ATTRIBUTE,
      (
        _,
        space: string,
        name: string,
        equals: string,
        quote: string,
        value: string,
      ) => `${space}${name}${equals}${quote}${rewrite(name, value)}${quote}`,
    );
  }

  // References may precede their definitions (including the root's ARIA list).
  svg.replace(TAG, (tag) =>
    mapAttributes(tag, (name, value) => {
      if (name === 'id' && !ids.has(value)) {
        ids.set(value, `${scope}-${ids.size}`);
      }
      return value;
    }),
  );

  return svg.replace(TAG, (tag) =>
    mapAttributes(tag, (name, value) => {
      if (name === 'id') return ids.get(value) ?? value;
      if (name === 'aria-labelledby' || name === 'aria-describedby') {
        return value.replace(/\S+/g, (id) => ids.get(id) ?? id);
      }
      if (name === 'href' || name === 'xlink:href') {
        if (value.startsWith('#')) {
          const id = ids.get(value.slice(1));
          if (id) return `#${id}`;
        }
        return value;
      }
      if (!URL_ATTRIBUTES.has(name)) return value;
      return value.replace(
        URL_REFERENCE,
        (reference, quote: string, id: string) => {
          const scoped = ids.get(id);
          return scoped ? `url(${quote}#${scoped}${quote})` : reference;
        },
      );
    }),
  );
}
