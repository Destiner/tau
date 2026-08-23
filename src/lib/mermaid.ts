import { shallowRef } from 'vue';

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
 */
const THEME = {
  bg: 'var(--sunk)',
  fg: 'var(--text)',
  line: 'var(--faint)',
  accent: 'var(--text)',
  muted: 'var(--muted)',
  surface: 'var(--panel-raised)',
  border: 'var(--border)',
} as const;

const OPTIONS = {
  ...THEME,
  // The app's own typeface, which it bundles. The library asks for the name
  // over the network as well; `withoutRemoteFonts` takes that request out.
  font: 'Inter Variable',
  // The block's own surface is drawn by its wrapper, so the canvas is only
  // the room the strokes at its edge need.
  transparent: true,
  padding: 12,
} as const;

/**
 * Laying out a diagram is graph work, so its cost is in the nodes rather than
 * the characters: this is the length past which a source is more likely to be
 * a data file that named itself a diagram than a diagram.
 */
const MAX_LENGTH = 8_000;

/**
 * Re-mounting a virtualized row re-renders its markdown, so the same diagrams
 * are asked for over and over. Fewer entries than the highlighter holds — a
 * diagram is rarer than a fenced block, and each one costs more to keep.
 */
const MAX_CACHED = 50;

/** Fence tags that ask for a diagram to be drawn rather than for its source. */
const TAGS = new Set(['mermaid']);

/** A remote font request, which a desktop app makes for nothing. */
const FONT_IMPORT = /^[ \t]*@import[^\n]*\n/gm;

/*
 * Attributes are space-separated in the library's output, which is what tells
 * an id from the tail of another attribute's name.
 */
const ID = / id="([^"]*)"/g;

const ID_REFERENCE = /url\(#([^)]*)\)/g;

const LANGUAGE_TAG = /^\S+/;

const cache = new Map<string, string>();

/** Distinguishes one diagram's ids from the next one's. */
let scopes = 0;

/*
 * Rendering is synchronous once the renderer is here, and markdown is rendered
 * synchronously, so the renderer is a ref: a diagram asked for before it
 * arrives stays its own source, and reading the ref while rendering is what
 * draws it once the renderer has landed.
 */
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

/**
 * Takes out the webfont the library asks the network for. The app bundles the
 * typeface it names, and a diagram in a transcript is not worth a request to
 * anyone — least of all one the reader did not ask for.
 */
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

  let svg: string;
  try {
    svg = render(source, OPTIONS);
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
