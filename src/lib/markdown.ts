import DOMPurify from "dompurify";
import { marked } from "marked";

marked.use({ gfm: true, breaks: true });

export interface MarkdownOptions {
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

export interface FileReference {
  /** The part of the candidate that forms the reference, line suffix included. */
  text: string;
  /** The path itself, as written, without any `:line:column` suffix. */
  path: string;
}

/** Marks the anchors this module writes, and the only ones a click opens. */
export const FILE_PATH_ATTRIBUTE = "data-tau-path";

/**
 * A run of the characters a path is written with, plus a `:line:column` tail.
 * Whitespace ends the run, so a path written with a space in it is read as the
 * two paths it looks like, the way a terminal reads one.
 */
const PATH_CANDIDATE = /[A-Za-z0-9~._/][A-Za-z0-9~._+@:/-]*/g;

/** Text inside these is a link already or quoted verbatim, so it is left alone. */
const OPAQUE_ELEMENTS = new Set(["a", "pre"]);

const TAG = /<\/?([A-Za-z][^\s/>]*)[^>]*>/g;

const LINE_SUFFIX = /^(.+?):\d+(?::\d+)?$/;

const TRAILING_PUNCTUATION = /[.,;:!?]+$/;

const ROOTED_PATH = /^(?:\/|\.{1,2}\/|~\/)/;

const NAMED_FILE = /\/[^/]*\.[A-Za-z0-9]{1,10}$/;

export function renderMarkdown(
  source: string,
  options: MarkdownOptions = {},
): string {
  const parsed = options.inline
    ? marked.parseInline(source, { async: false })
    : marked.parse(source, { async: false });
  // The attribute belongs to this module: text that arrives already carrying
  // one cannot pass itself off as a file the app resolved.
  const html = DOMPurify.sanitize(parsed as string, {
    FORBID_ATTR: [FILE_PATH_ATTRIBUTE],
  });
  return options.basePath ? linkFilePaths(html) : html;
}

/** Rewrites the file paths in rendered markup as links, leaving markup alone. */
export function linkFilePaths(html: string): string {
  let output = "";
  let plainFrom = 0;
  let opaque: string | null = null;
  let depth = 0;

  TAG.lastIndex = 0;
  for (let tag = TAG.exec(html); tag; tag = TAG.exec(html)) {
    const text = html.slice(plainFrom, tag.index);
    output += opaque ? text : linkTextRun(text);
    output += tag[0];
    plainFrom = tag.index + tag[0].length;

    const name = tag[1].toLowerCase();
    const closing = tag[0].startsWith("</");
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
export function parseFileReference(candidate: string): FileReference | null {
  const text = candidate.replace(TRAILING_PUNCTUATION, "");
  const path = LINE_SUFFIX.exec(text)?.[1] ?? text;

  if (!path.includes("/") || path.includes("//")) return null;

  const rooted = ROOTED_PATH.test(path);
  // A root on its own names no file, whoever it belongs to.
  const body = rooted ? path.replace(ROOTED_PATH, "") : path;
  if (body.split("/").filter(Boolean).length === 0) return null;

  const directory = path.endsWith("/");
  if (!rooted && !directory && !NAMED_FILE.test(path)) return null;

  return { text, path };
}

/** Resolves a written path against the session's directory and the home one. */
export function resolveFilePath(
  basePath: string,
  path: string,
  home?: string,
): string {
  if (path.startsWith("~/")) {
    return home ? normalizePath(`${home}/${path.slice(2)}`) : path;
  }
  if (path.startsWith("/")) return normalizePath(path);
  return normalizePath(`${basePath}/${path}`);
}

export function isWebUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function linkTextRun(text: string): string {
  PATH_CANDIDATE.lastIndex = 0;
  return text.replace(PATH_CANDIDATE, (candidate) => {
    const reference = parseFileReference(candidate);
    if (!reference) return candidate;
    // Whatever the reference stopped short of is punctuation, not the path.
    const trailing = candidate.slice(reference.text.length);
    const path = reference.path.replace(/"/g, "&quot;");
    return `<a class="file-link" role="link" tabindex="0" ${FILE_PATH_ATTRIBUTE}="${path}">${reference.text}</a>${trailing}`;
  });
}

function normalizePath(path: string): string {
  const rooted = path.startsWith("/");
  const segments: string[] = [];

  for (const segment of path.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment !== "..") {
      segments.push(segment);
      continue;
    }
    const parent = segments[segments.length - 1];
    if (parent && parent !== "..") segments.pop();
    else if (!rooted) segments.push("..");
  }

  return (rooted ? "/" : "") + segments.join("/");
}
