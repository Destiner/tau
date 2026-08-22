import purify from 'dompurify';
import { marked } from 'marked';

marked.use({ gfm: true, breaks: true });

interface MarkdownOptions {
  /**
   * Renders a single run of text: emphasis, code, and links, but no headings,
   * lists, or quotes. A prompt's title is a sentence rather than a document.
   */
  inline?: boolean;
  /**
   * Directory that relative file paths are resolved against. Passing it turns
   * the file paths in the text into links; a remote project's files are not on
   * this machine, so it passes nothing and its paths stay as text.
   */
  basePath?: string;
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

/** A fenced block, whose end is unambiguous because `pre` cannot nest. */
const CODE_BLOCK = /<pre\b[^>]*>[\s\S]*?<\/pre>/g;

/*
 * The button lives inside sanitized HTML rather than in the component tree, so
 * its icons are markup here instead of a `UiIcon`. Both are drawn and the
 * stylesheet picks one, which keeps a copy down to a single attribute write.
 */
const COPY_ICON =
  '<svg class="code-copy-idle" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M3.5 6.5h10v10h-10zM6.5 6.5v-3h10v10h-3"/></svg>';

const COPIED_ICON =
  '<svg class="code-copy-done" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="m4.5 10.5 4 4 7-8"/></svg>';

/**
 * A path embedded in prose, plus a `:line:column` tail. Whitespace ends these
 * candidates; a rooted path that fills its rendered line is handled separately
 * so its spaces and punctuation are unambiguous.
 */
const PATH_CANDIDATE = /[A-Za-z0-9~._/][A-Za-z0-9~._+@:/-]*/g;

/** Existing links and fenced code blocks are left exactly as written. */
const OPAQUE_ELEMENTS = new Set(['a', 'pre']);

const APPLE_PLATFORM = /^(?:Mac|iPhone|iPad|iPod)/;

const TAG = /<\/?([A-Za-z][^\s/>]*)[^>]*>/g;

const LINE_SUFFIX = /^(.+?):\d+(?::\d+)?$/;

const TRAILING_PUNCTUATION = /[.,;:!?]+$/;

const ROOTED_PATH = /^(?:\/|\.{1,2}\/|~\/)/;

const NAMED_FILE = /\/[^/]*\.[A-Za-z0-9]{1,10}$/;

function renderMarkdown(source: string, options: MarkdownOptions = {}): string {
  const parsed = options.inline
    ? marked.parseInline(source, { async: false })
    : marked.parse(source, { async: false });
  // The attribute belongs to this module: text that arrives already carrying
  // one cannot pass itself off as a file the app resolved.
  const html = purify.sanitize(parsed as string, {
    FORBID_ATTR: [FILE_PATH_ATTRIBUTE, CODE_COPY_ATTRIBUTE],
  });
  const linked = options.basePath ? linkFilePaths(html) : html;
  return options.inline ? linked : addCodeCopyButtons(linked);
}

/** Gives every fenced block a copy button, positioned against the wrapper so
 * that scrolling a wide block sideways does not carry the button off. */
function addCodeCopyButtons(html: string): string {
  CODE_BLOCK.lastIndex = 0;
  return html.replace(
    CODE_BLOCK,
    (block) =>
      `<div class="code-block">${block}<button type="button" class="code-copy" ${CODE_COPY_ATTRIBUTE} aria-label="Copy code">${COPY_ICON}${COPIED_ICON}</button></div>`,
  );
}

/** Rewrites the file paths in rendered markup as links, leaving markup alone. */
function linkFilePaths(html: string): string {
  let output = '';
  let plainFrom = 0;
  let opaque: string | null = null;
  let depth = 0;

  TAG.lastIndex = 0;
  for (let tag = TAG.exec(html); tag; tag = TAG.exec(html)) {
    const text = html.slice(plainFrom, tag.index);
    output += opaque ? text : linkTextRun(text);
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
  return output + (opaque ? tail : linkTextRun(tail));
}

/**
 * Reads a candidate as a file reference, or rejects it. A path is either
 * rooted, or names a file with an extension, so that prose like `and/or` and
 * `24/7` stays prose.
 */
function parseFileReference(candidate: string): FileReference | null {
  const text = candidate.replace(TRAILING_PUNCTUATION, '');
  const path = LINE_SUFFIX.exec(text)?.[1] ?? text;

  if (!path.includes('/') || path.includes('//')) return null;

  const rooted = ROOTED_PATH.test(path);
  // A root on its own names no file, whoever it belongs to.
  const body = rooted ? path.replace(ROOTED_PATH, '') : path;
  if (body.split('/').filter(Boolean).length === 0) return null;

  const directory = path.endsWith('/');
  if (!rooted && !directory && !NAMED_FILE.test(path)) return null;

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

/** Matches the desktop convention: Command-click on Apple, Control-click elsewhere. */
function isPathOpenGesture(event: PathOpenGesture, platform: string): boolean {
  if (event.type === 'keydown') return event.key === 'Enter';
  if (event.type !== 'click' || event.button !== 0) return false;
  return APPLE_PLATFORM.test(platform)
    ? event.metaKey === true
    : event.ctrlKey === true;
}

function linkTextRun(text: string): string {
  return text
    .split(/(\r?\n)/)
    .map((line) => linkTextLine(line))
    .join('');
}

function linkTextLine(text: string): string {
  const standalone = linkStandaloneRootedPath(text);
  if (standalone) return standalone;

  PATH_CANDIDATE.lastIndex = 0;
  return text.replace(PATH_CANDIDATE, (candidate) => {
    const reference = parseFileReference(candidate);
    if (!reference) return candidate;
    // Whatever the reference stopped short of is punctuation, not the path.
    const trailing = candidate.slice(reference.text.length);
    return fileLink(reference.path, reference.text) + trailing;
  });
}

/** A whole rooted path has a clear end even when its name contains spaces. */
function linkStandaloneRootedPath(text: string): string | null {
  const leading = /^\s*/.exec(text)?.[0] ?? '';
  const trailing = /\s*$/.exec(text)?.[0] ?? '';
  const end = text.length - trailing.length;
  const encodedCandidate = text.slice(leading.length, end);
  if (!encodedCandidate) return null;

  const candidate = decodeHtmlText(encodedCandidate);
  if (!ROOTED_PATH.test(candidate)) return null;

  const reference = parseFileReference(candidate);
  if (!reference || reference.text !== candidate) return null;
  return `${leading}${fileLink(reference.path, encodedCandidate)}${trailing}`;
}

function fileLink(path: string, text: string): string {
  return `<a class="file-link" role="link" tabindex="0" ${FILE_PATH_ATTRIBUTE}="${escapeHtmlAttribute(path)}">${text}</a>`;
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
  FILE_PATH_ATTRIBUTE,
  addCodeCopyButtons,
  renderMarkdown,
  linkFilePaths,
  parseFileReference,
  resolveFilePath,
  isWebUrl,
  isPathOpenGesture,
};
