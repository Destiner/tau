import purify from 'dompurify';
import { marked, type Tokens } from 'marked';

import highlightCode from './highlight';
import renderDiagram from './mermaid';

marked.use({ gfm: true, breaks: true, renderer: { code: renderCode } });

interface MarkdownOptions {
  /**
   * Renders a single run of text: emphasis, code, and links, but no headings,
   * lists, or quotes. A prompt's title is a sentence rather than a document.
   */
  inline?: boolean;
  /** Directory that relative file paths are resolved against. */
  basePath?: string;
  /** Remote paths are links that copy instead of opening on this machine. */
  copyPaths?: boolean;
}

interface FileReference {
  /** The part of the candidate that forms the reference, line suffix included. */
  text: string;
  /** The path itself, as written, without any `:line:column` suffix. */
  path: string;
}

interface PathOpenGesture {
  type: string;
  key?: string;
  button?: number;
  metaKey?: boolean;
  ctrlKey?: boolean;
}

/** Marks the anchors this module writes, and the only ones a click opens. */
const FILE_PATH_ATTRIBUTE = 'data-tau-path';

/** Marks the copy buttons this module writes, and the only ones a click copies. */
const CODE_COPY_ATTRIBUTE = 'data-tau-copy';

/** Marks the expand buttons this module writes, and the only ones a click expands. */
const DIAGRAM_EXPAND_ATTRIBUTE = 'data-tau-expand';

/** Names a wrapped block's language for the label its stylesheet draws. */
const CODE_LANGUAGE_ATTRIBUTE = 'data-tau-lang';

/** A fenced block, whose end is unambiguous because `pre` cannot nest. */
const CODE_BLOCK = /<pre\b[^>]*>[\s\S]*?<\/pre>/g;

/** The language a block was fenced with, as marked and Shiki both write it. */
const CODE_LANGUAGE = /<code[^>]*\bclass="(?:[^"]*\s)?language-([^"\s]+)/;

/*
 * The button lives inside sanitized HTML rather than in the component tree, so
 * its icons are markup here instead of a `UiIcon`. Both are drawn and the
 * stylesheet picks one, which keeps a copy down to a single attribute write.
 * Phosphor regular paths, verbatim.
 */
const COPY_ICON =
  '<svg class="copy" viewBox="0 0 256 256" fill="currentColor" aria-hidden="true"><path d="M216,32H88a8,8,0,0,0-8,8V80H40a8,8,0,0,0-8,8V216a8,8,0,0,0,8,8H168a8,8,0,0,0,8-8V176h40a8,8,0,0,0,8-8V40A8,8,0,0,0,216,32ZM160,208H48V96H160Zm48-48H176V88a8,8,0,0,0-8-8H96V48H208Z"/></svg>';

const COPIED_ICON =
  '<svg class="check" viewBox="0 0 256 256" fill="currentColor" aria-hidden="true"><path d="M229.66,77.66l-128,128a8,8,0,0,1-11.32,0l-56-56a8,8,0,0,1,11.32-11.32L96,188.69,218.34,66.34a8,8,0,0,1,11.32,11.32Z"/></svg>';

const EXPAND_ICON =
  '<svg viewBox="0 0 256 256" fill="currentColor" aria-hidden="true"><path d="M216,48V88a8,8,0,0,1-16,0V56H168a8,8,0,0,1,0-16h40A8,8,0,0,1,216,48ZM88,200H56V168a8,8,0,0,0-16,0v40a8,8,0,0,0,8,8H88a8,8,0,0,0,0-16Zm120-40a8,8,0,0,0-8,8v32H168a8,8,0,0,0,0,16h40a8,8,0,0,0,8-8V168A8,8,0,0,0,208,160ZM88,40H48a8,8,0,0,0-8,8V88a8,8,0,0,0,16,0V56H88a8,8,0,0,0,0-16Z"/></svg>';

/**
 * A drawn diagram's wrapper, holding exactly the svg the renderer drew. The
 * lazy close is safe for the renderer's output, which never nests an svg.
 */
const DIAGRAM_BLOCK = /<div class="diagram"><svg[\s\S]*?<\/svg><\/div>/g;

/**
 * A path embedded in prose, plus a `:line:column` tail. Whitespace ends these
 * candidates; a rooted path that fills its rendered line is handled separately
 * so its spaces and punctuation are unambiguous.
 */
const PATH_CANDIDATE = /[A-Za-z0-9~._/][A-Za-z0-9~._+@:/-]*/g;

/** Existing links and diagrams are left exactly as written. */
const OPAQUE_ELEMENTS = new Set(['a', 'pre', 'svg']);

const APPLE_PLATFORM = /^(?:Mac|iPhone|iPad|iPod)/;

const TAG = /<\/?([A-Za-z][^\s/>]*)[^>]*>/g;

const LINE_SUFFIX = /^(.+?):\d+(?::\d+)?$/;

const TRAILING_PUNCTUATION = /[.,;:!?]+$/;

const ROOTED_PATH = /^(?:\/|\.{1,2}\/|~\/)/;

const NAMED_FILE = /\/[^/]*\.[A-Za-z0-9]{1,10}$/;

const HTML_ENTITY_PREFIX = /^&(?:amp|quot|apos|lt|gt|#\d+|#x[\da-f]+);/i;

const HTML_ENTITY_SUFFIX = /&(?:amp|quot|apos|lt|gt|#\d+|#x[\da-f]+);$/i;

const EDGE_WHITESPACE = /^\s|\s$/;

const ROUTE_TEMPLATE_PUNCTUATION = /[{}*[\]:]/;

const PARAMETER_SEGMENT = /^[:*]/;

/** A fence's closing run, which the source of an unfinished block has not reached. */
const CLOSING_FENCE = /(?:^|\n)[ \t]*(?:`{3,}|~{3,})$/;

/** A GFM table has one header row; only a row with no rendered cell content is omitted. */
const EMPTY_TABLE_HEADER =
  /^\s*<tr\b[^>]*>(?:\s*<th\b[^>]*>\s*<\/th>\s*)+<\/tr>\s*$/;

function renderMarkdown(source: string, options: MarkdownOptions = {}): string {
  const parsed = options.inline
    ? marked.parseInline(source, { async: false })
    : marked.parse(source, { async: false });
  // These attributes belong to this module: text that arrives already carrying
  // one cannot pass itself off as something the app wrote about it.
  const html = purify.sanitize(parsed as string, {
    FORBID_ATTR: [
      FILE_PATH_ATTRIBUTE,
      CODE_COPY_ATTRIBUTE,
      CODE_LANGUAGE_ATTRIBUTE,
      DIAGRAM_EXPAND_ATTRIBUTE,
    ],
  });
  const withoutEmptyTableHeaders = removeEmptyTableHeaders(html);
  const linked =
    options.basePath || options.copyPaths
      ? linkFilePaths(withoutEmptyTableHeaders, options.copyPaths)
      : withoutEmptyTableHeaders;
  return options.inline
    ? linked
    : addDiagramExpandButtons(addCodeCopyButtons(linked));
}

/**
 * GFM requires a header delimiter even when a table is really a list of rows.
 * Marked correctly emits that empty row as a thead; remove it after sanitizing
 * so alignment on the body cells and every non-empty header stay untouched.
 */
function removeEmptyTableHeaders(html: string): string {
  return html.replace(
    /<thead\b[^>]*>([\s\S]*?)<\/thead>/g,
    (section, contents) => (EMPTY_TABLE_HEADER.test(contents) ? '' : section),
  );
}

/**
 * Draws a fenced diagram or highlights a fenced block, or leaves marked to
 * render it as it always has: a language we hold no grammar for is still
 * perfectly readable code, and so is a diagram that cannot be drawn.
 */
function renderCode(token: Tokens.Code): string | false {
  if (!token.lang) return false;
  // marked's own output ends a block with a newline, and a block copied out of
  // the transcript should still end in one.
  const code = `${token.text.replace(/\n$/, '')}\n`;

  // Only a closed fence is a whole diagram. A streamed one arrives a line at a
  // time, and drawing each prefix would lay out a diagram per delta and move
  // the reader's page around under a picture that keeps changing shape.
  if (isClosedFence(token.raw)) {
    const diagram = renderDiagram(code, token.lang);
    if (diagram) return `<div class="diagram">${diagram}</div>`;
  }

  return highlightCode(code, token.lang) ?? false;
}

/** Whether a fenced block's source reached its closing fence. */
function isClosedFence(raw: string): boolean {
  return CLOSING_FENCE.test(raw.trimEnd());
}

/** Gives every fenced block a copy button, positioned against the wrapper so
 * that scrolling a wide block sideways does not carry the button off. The
 * language rides along on the wrapper because the label is drawn from it. */
function addCodeCopyButtons(html: string): string {
  CODE_BLOCK.lastIndex = 0;
  return html.replace(CODE_BLOCK, (block) => {
    const language = CODE_LANGUAGE.exec(block)?.[1];
    const label = language
      ? ` ${CODE_LANGUAGE_ATTRIBUTE}="${escapeHtmlAttribute(language)}"`
      : '';
    return `<div class="code-block"${label}>${block}<button type="button" class="code-copy" ${CODE_COPY_ATTRIBUTE} aria-label="Copy Code">${COPY_ICON}${COPIED_ICON}</button></div>`;
  });
}

/** Gives every drawn diagram an expand button on the copy button's terms:
 * against the wrapper, and injected after sanitizing so the attribute marks
 * buttons the app wrote rather than ones the text brought with it. */
function addDiagramExpandButtons(html: string): string {
  DIAGRAM_BLOCK.lastIndex = 0;
  return html.replace(DIAGRAM_BLOCK, (block) =>
    block.replace(
      /<\/div>$/,
      `<button type="button" class="diagram-expand" ${DIAGRAM_EXPAND_ATTRIBUTE} aria-label="Expand Diagram">${EXPAND_ICON}</button></div>`,
    ),
  );
}

/** Rewrites file paths in rendered prose and inline code, leaving opaque markup alone. */
function linkFilePaths(html: string, copyPaths = false): string {
  let output = '';
  let plainFrom = 0;
  let opaque: string | null = null;
  let depth = 0;

  TAG.lastIndex = 0;
  for (let tag = TAG.exec(html); tag; tag = TAG.exec(html)) {
    const text = html.slice(plainFrom, tag.index);
    output += opaque ? text : linkTextRun(text, copyPaths);
    output += tag[0];
    plainFrom = tag.index + tag[0].length;

    const name = (tag[1] ?? '').toLowerCase();
    const closing = tag[0].startsWith('</');
    if (opaque === name) {
      depth += closing ? -1 : 1;
      if (depth === 0) opaque = null;
    } else if (!opaque && !closing && OPAQUE_ELEMENTS.has(name)) {
      opaque = name;
      depth = 1;
    }
  }

  const tail = html.slice(plainFrom);
  return output + (opaque ? tail : linkTextRun(tail, copyPaths));
}

/**
 * Reads a candidate as a file reference, or rejects it. A path is either
 * rooted, or names a file with an extension, so that prose like `and/or` and
 * `24/7` stays prose.
 */
function parseFileReference(candidate: string): FileReference | null {
  const text = candidate.replace(TRAILING_PUNCTUATION, '');
  const path = LINE_SUFFIX.exec(text)?.[1] ?? text;

  if (!path.includes('/') || path.includes('//') || hasControlCharacter(path)) {
    return null;
  }

  const rooted = ROOTED_PATH.test(path);
  // Segment-edge spaces are legal on disk but much more likely to be prose.
  const body = rooted ? path.replace(ROOTED_PATH, '') : path;
  const segments = body.split('/').filter(Boolean);
  if (
    segments.length === 0 ||
    segments.some((segment) => EDGE_WHITESPACE.test(segment))
  ) {
    return null;
  }

  const directory = path.endsWith('/');
  const namedFile = NAMED_FILE.test(path);
  if (!rooted && !directory && !namedFile) return null;
  // An implicit one-word directory is more often a count, label, or prose.
  if (!rooted && directory && segments.length < 2) return null;
  // A lone rooted word is also how slash commands and markup tags are written.
  if (rooted && segments.length === 1 && !directory && !namedFile) return null;
  // Route parameters and brace alternatives describe URL shapes, not one file.
  if (
    ROUTE_TEMPLATE_PUNCTUATION.test(path) ||
    segments.some((segment) => PARAMETER_SEGMENT.test(segment))
  ) {
    return null;
  }

  return { text, path };
}

/** Resolves a written path against the session's directory and the home one. */
function resolveFilePath(
  basePath: string,
  path: string,
  home?: string,
): string {
  if (path.startsWith('~/')) {
    return home ? normalizePath(`${home}/${path.slice(2)}`) : path;
  }
  if (path.startsWith('/')) return normalizePath(path);
  return normalizePath(`${basePath}/${path}`);
}

function isWebUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch {
    return false;
  }
}

/** Reads an explicit Markdown destination as a path, without applying prose heuristics. */
function parseMarkdownFileDestination(value: string): string | null {
  if (!value || value.startsWith('#') || hasControlCharacter(value))
    return null;
  if (/^[A-Za-z][A-Za-z\d+.-]*:/.test(value)) {
    if (!value.startsWith('file:')) return null;
    try {
      const url = new URL(value);
      if (url.hostname && url.hostname !== 'localhost') return null;
      return decodeURIComponent(url.pathname);
    } catch {
      return null;
    }
  }
  try {
    const decoded = decodeURIComponent(value);
    return decoded && !hasControlCharacter(decoded) ? decoded : null;
  } catch {
    return null;
  }
}

/** Ordinary primary click and Enter activate paths; Control-click remains a menu gesture. */
function isPathOpenGesture(event: PathOpenGesture, platform: string): boolean {
  if (event.type === 'keydown') return event.key === 'Enter';
  if (event.type !== 'click' || event.button !== 0) return false;
  return !(APPLE_PLATFORM.test(platform) && event.ctrlKey === true);
}

function linkTextRun(text: string, copyPaths: boolean): string {
  return text
    .split(/(\r?\n)/)
    .map((line) => linkTextLine(line, copyPaths))
    .join('');
}

function linkTextLine(text: string, copyPaths: boolean): string {
  const standalone = linkStandaloneRootedPath(text, copyPaths);
  if (standalone) return standalone;

  PATH_CANDIDATE.lastIndex = 0;
  return text.replace(PATH_CANDIDATE, (candidate, offset: number) => {
    if (
      touchesHtmlEntity(text, offset, candidate.length) ||
      touchesRouteTemplate(text, offset, candidate.length)
    ) {
      return candidate;
    }

    const reference = parseFileReference(candidate);
    if (!reference) return candidate;
    // Whatever the reference stopped short of is punctuation, not the path.
    const trailing = candidate.slice(reference.text.length);
    return fileLink(reference.path, reference.text, copyPaths) + trailing;
  });
}

function hasControlCharacter(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 0x1f || codePoint === 0x7f;
  });
}

/** A template marker outside the candidate still belongs to the route shape. */
function touchesRouteTemplate(
  text: string,
  offset: number,
  length: number,
): boolean {
  return (
    ROUTE_TEMPLATE_PUNCTUATION.test(text[offset - 1] ?? '') ||
    ROUTE_TEMPLATE_PUNCTUATION.test(text[offset + length] ?? '')
  );
}

/** Encoded markup punctuation does not turn the text beside it into a path. */
function touchesHtmlEntity(
  text: string,
  offset: number,
  length: number,
): boolean {
  return (
    HTML_ENTITY_SUFFIX.test(text.slice(0, offset)) ||
    HTML_ENTITY_PREFIX.test(text.slice(offset + length))
  );
}

/** A whole rooted path has a clear end even when its name contains spaces. */
function linkStandaloneRootedPath(
  text: string,
  copyPaths: boolean,
): string | null {
  const leading = /^\s*/.exec(text)?.[0] ?? '';
  const trailing = /\s*$/.exec(text)?.[0] ?? '';
  const end = text.length - trailing.length;
  const encodedCandidate = text.slice(leading.length, end);
  if (!encodedCandidate) return null;

  const candidate = decodeHtmlText(encodedCandidate);
  if (!ROOTED_PATH.test(candidate)) return null;

  const reference = parseFileReference(candidate);
  if (!reference || reference.text !== candidate) return null;
  return `${leading}${fileLink(reference.path, encodedCandidate, copyPaths)}${trailing}`;
}

function fileLink(path: string, text: string, copyPath = false): string {
  const escapedPath = escapeHtmlAttribute(path);
  const semantics = copyPath
    ? `role="button" tabindex="0" aria-label="Preview path ${escapedPath}"`
    : 'role="link" tabindex="0"';
  return `<a class="file-link" ${semantics} ${FILE_PATH_ATTRIBUTE}="${escapedPath}">${text}</a>`;
}

function decodeHtmlText(value: string): string {
  return value.replace(
    /&(?:amp|quot|apos|lt|gt|#\d+|#x[\da-f]+);/gi,
    (entity) => {
      const name = entity.slice(1, -1).toLowerCase();
      if (name === 'amp') return '&';
      if (name === 'quot') return '"';
      if (name === 'apos') return "'";
      if (name === 'lt') return '<';
      if (name === 'gt') return '>';

      const hexadecimal = name.startsWith('#x');
      const digits = name.slice(hexadecimal ? 2 : 1);
      const codePoint = Number.parseInt(digits, hexadecimal ? 16 : 10);
      return codePoint <= 0x10ffff ? String.fromCodePoint(codePoint) : entity;
    },
  );
}

function escapeHtmlAttribute(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function normalizePath(path: string): string {
  const rooted = path.startsWith('/');
  const segments: string[] = [];

  for (const segment of path.split('/')) {
    if (!segment || segment === '.') continue;
    if (segment !== '..') {
      segments.push(segment);
      continue;
    }
    const parent = segments[segments.length - 1];
    if (parent && parent !== '..') segments.pop();
    else if (!rooted) segments.push('..');
  }

  return (rooted ? '/' : '') + segments.join('/');
}

export type { MarkdownOptions, FileReference, PathOpenGesture };

export {
  CODE_COPY_ATTRIBUTE,
  CODE_LANGUAGE_ATTRIBUTE,
  DIAGRAM_EXPAND_ATTRIBUTE,
  FILE_PATH_ATTRIBUTE,
  addCodeCopyButtons,
  addDiagramExpandButtons,
  renderMarkdown,
  isClosedFence,
  linkFilePaths,
  removeEmptyTableHeaders,
  parseFileReference,
  parseMarkdownFileDestination,
  resolveFilePath,
  isWebUrl,
  isPathOpenGesture,
};
