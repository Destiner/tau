import { shallowRef } from 'vue';

import { adaptMermaidSource } from './mermaid-source';

type DiagramRenderer = typeof import('beautiful-mermaid').renderMermaidSVG;

/**
 * Ayu, in whichever scheme the system is in. beautiful-mermaid ships no Ayu
 * theme, and a pair of them written out here would be a second copy of the
 * palette; every colour is instead a reference to the token the app already
 * draws itself with, so a diagram follows an appearance change by cascade
 * rather than by being drawn again.
 *
 * All seven are given, including the ones a diagram would otherwise derive
 * from `bg` and `fg`: the library reads its optional colours from `--accent`,
 * `--muted`, and `--border`, which are names Tau's own tokens already carry,
 * so anything left underived would quietly inherit the app's value for it.
 *
 * `accent` is the arrow heads, and it is the text colour rather than Tau's
 * accent: the accent means attention somewhere in the app, and every arrow in
 * a diagram is not it.
 *
 * `bg` is the surface a diagram is drawn on rather than one it draws: nothing
 * is painted behind it, and the library mixes its faint washes towards this.
 * The canvas is what a message sits on; a bubble's own wash is near enough to
 * it that a 12% mix cannot tell them apart. `surface` is what lifts a node off
 * that page now that the diagram has no block of its own to sit in.
 */
const THEME = {
  bg: 'var(--canvas)',
  fg: 'var(--text)',
  line: 'var(--faint)',
  accent: 'var(--text)',
  muted: 'var(--muted)',
  surface: 'var(--panel-raised)',
  border: 'var(--border)',
} as const;

const OPTIONS = {
  ...THEME,

  font: 'Inter Variable',

  transparent: true,

  padding: 12,
} as const;

const MAX_LENGTH = 8_000;

const MAX_CACHED = 50;

const TAGS = new Set(['mermaid']);

const FONT_IMPORT = /^[ \t]*@import[^\n]*\n/gm;

const ID = / id="([^"]*)"/g;

const ID_REFERENCE = /url\(#([^)]*)\)/g;

const LANGUAGE_TAG = /^\S+/;

const cache = new Map<string, string>();

let scopes = 0;

const renderer = shallowRef<DiagramRenderer | null>(null);
let loading = false;

/**
 * Nothing about diagrams is loaded until a transcript holds one: the layout
 * engine is over a megabyte, and most sessions never mention a diagram.
 */
function load(): void {
  if (loading) return;
  loading = true;
  void import('beautiful-mermaid')
    .then((module) => {
      renderer.value = module.renderMermaidSVG;
    })
    .catch((error: unknown) => {
      console.error('Could not load the diagram renderer', error);
    });
}

function isDiagram(fence: string): boolean {
  const tag = LANGUAGE_TAG.exec(fence)?.[0].toLowerCase();
  return tag !== undefined && TAGS.has(tag);
}

function withoutRemoteFonts(svg: string): string {
  FONT_IMPORT.lastIndex = 0;
  return svg.replace(FONT_IMPORT, '');
}

/**
 * Marks a diagram's ids as its own. Arrow heads are referenced by id, and the
 * ids of everything else are the diagram's node names: two diagrams in one
 * transcript would otherwise share whichever one the document holds first,
 * and a node named after part of the app would collide with the app itself.
 */
function scopeIds(svg: string, scope: string): string {
  ID.lastIndex = 0;
  ID_REFERENCE.lastIndex = 0;
  return svg
    .replace(ID, (_, id: string) => ` id="${scope}-${id}"`)
    .replace(ID_REFERENCE, (_, id: string) => `url(#${scope}-${id})`);
}

function remember(key: string, svg: string): void {
  if (cache.size >= MAX_CACHED) {
    const oldest = cache.keys().next();
    if (!oldest.done) cache.delete(oldest.value);
  }
  cache.set(key, svg);
}

/**
 * Draws a fenced diagram, or reports that the fence stays the source it was
 * written as. A renderer that has not loaded yet, a fence that asked for
 * something else, an oversized source, and a diagram the parser cannot read
 * all mean the same thing to the caller.
 */
function renderDiagram(source: string, fence: string): string | null {
  if (!isDiagram(fence)) return null;

  const render = renderer.value;
  if (!render) {
    load();
    return null;
  }
  if (source.length > MAX_LENGTH) return null;

  const cached = cache.get(source);
  if (cached !== undefined) return cached;

  const adapted = adaptMermaidSource(source);
  if (adapted.kind === 'unsafe') return null;

  let svg: string;
  try {
    svg = render(adapted.source, OPTIONS);
  } catch (error) {
    // A diagram that cannot be read is content rather than a failure, and the
    // source the reader was sent is what it falls back to. Half of a diagram
    // is unreadable by definition, so a streamed one arrives this way.
    console.debug('Could not draw the diagram', error);
    return null;
  }

  scopes += 1;
  const scoped = scopeIds(withoutRemoteFonts(svg), `diagram-${scopes}`);
  remember(source, scoped);
  return scoped;
}

export default renderDiagram;
