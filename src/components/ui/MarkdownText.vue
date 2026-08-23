<template>
  <!-- eslint-disable vue/no-v-html -- renderMarkdown sanitizes with DOMPurify -->
  <div
    class="markdown"
    @click="activate"
    @keydown="handleKeydown"
    v-html="rendered"
  ></div>
  <!-- eslint-enable vue/no-v-html -->
</template>

<script setup lang="ts">
import { homeDir } from '@tauri-apps/api/path';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { openPath, openUrl } from '@tauri-apps/plugin-opener';
import { computed, onBeforeUnmount } from 'vue';

import {
  CODE_COPY_ATTRIBUTE,
  FILE_PATH_ATTRIBUTE,
  isPathOpenGesture,
  isWebUrl,
  parseFileReference,
  renderMarkdown,
  resolveFilePath,
} from '../../lib/markdown';

const props = defineProps<{
  source: string;
  inline?: boolean;
  basePath?: string;
}>();

const rendered = computed(() =>
  renderMarkdown(props.source, {
    ...(props.inline ? { inline: true } : {}),
    ...(props.basePath ? { basePath: props.basePath } : {}),
  }),
);

/** How long a copied block keeps saying so before the icon returns. */
const COPIED_FEEDBACK_MS = 1_200;

let homePath: Promise<string> | null = null;
let copiedTimer: ReturnType<typeof setTimeout> | undefined;
let copiedButton: HTMLElement | undefined;

/** Asked for once, and asked for again if it ever fails. */
function homeDirectory(): Promise<string> {
  if (!homePath) {
    homePath = homeDir().catch((error: unknown) => {
      homePath = null;
      throw error;
    });
  }
  return homePath;
}

function linkAt(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  const link = target.closest(`a[href], a[${FILE_PATH_ATTRIBUTE}]`);
  return link instanceof HTMLElement ? link : null;
}

/**
 * The path a link points at, whether it was written as one in the text or is a
 * markdown link that names a file rather than a page.
 */
function filePath(link: HTMLElement): string | null {
  const written = link.getAttribute(FILE_PATH_ATTRIBUTE);
  if (written) return written;

  const href = link.getAttribute('href');
  return href ? (parseFileReference(href)?.path ?? null) : null;
}

async function openLocalPath(path: string, basePath: string): Promise<void> {
  try {
    await openPath(resolveFilePath(basePath, path, await homeDirectory()));
  } catch (error) {
    console.error('Could not open the path', error);
  }
}

async function openWebLink(href: string): Promise<void> {
  try {
    await openUrl(href);
  } catch (error) {
    console.error('Could not open link in the default browser', error);
  }
}

function copyButtonAt(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  const button = target.closest(`button[${CODE_COPY_ATTRIBUTE}]`);
  return button instanceof HTMLElement ? button : null;
}

/** Marks the button that was just used, so the icon reports the copy landed. */
function showCopied(button: HTMLElement): void {
  clearCopied();
  copiedButton = button;
  button.dataset.copied = 'true';
  copiedTimer = setTimeout(clearCopied, COPIED_FEEDBACK_MS);
}

function clearCopied(): void {
  clearTimeout(copiedTimer);
  copiedTimer = undefined;
  delete copiedButton?.dataset.copied;
  copiedButton = undefined;
}

async function copyCodeBlock(button: HTMLElement): Promise<void> {
  const code = button.parentElement?.querySelector('pre')?.textContent;
  if (!code) return;
  try {
    await writeText(code);
  } catch (error) {
    console.error('Could not copy the code block', error);
    return;
  }
  showCopied(button);
}

async function activate(event: Event): Promise<void> {
  const copy = copyButtonAt(event.target);
  if (copy) {
    await copyCodeBlock(copy);
    return;
  }

  const link = linkAt(event.target);
  if (!link) return;

  // What was written, not what the webview resolved it to: a fragment or a
  // relative link would otherwise read as a link to the app's own address.
  const href = link.getAttribute('href');
  if (href && isWebUrl(href)) {
    event.preventDefault();
    await openWebLink(href);
    return;
  }

  const path = filePath(link);
  if (!path) return;

  // Never let a relative markdown link navigate the webview. Opening it is a
  // deliberate desktop gesture so an ordinary click can still place a caret.
  event.preventDefault();
  if (props.basePath && isPathOpenGesture(event, window.navigator.platform)) {
    await openLocalPath(path, props.basePath);
  }
}

function handleKeydown(event: KeyboardEvent): void {
  // A copy button is a button: the browser turns Enter and Space into a click,
  // so handling Enter here as well would copy twice.
  if (event.key === 'Enter' && !copyButtonAt(event.target))
    void activate(event);
}

onBeforeUnmount(clearCopied);
</script>

<style scoped>
/* The rendered HTML carries no scoped attributes, so its descendants are
 * reached with :deep; the class itself is Tau's only markdown surface. */
.markdown {
  color: var(--text);
  line-height: 1.62;
  cursor: text;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: text;
  user-select: text;
  overflow-wrap: anywhere;
}

.markdown :deep(> :first-child) {
  margin-top: 0;
}

.markdown :deep(> :last-child) {
  margin-bottom: 0;
}

.markdown :deep(p),
.markdown :deep(ul),
.markdown :deep(ol),
.markdown :deep(.code-block),
.markdown :deep(.diagram),
.markdown :deep(blockquote) {
  margin: 0.7em 0;
}

/*
 * Browser defaults indent a list about 40px, which is far wider than the
 * 0.7em rhythm the blocks around it keep; the marker column only has to be
 * wide enough to hold a marker.
 */
.markdown :deep(ul),
.markdown :deep(ol) {
  padding-left: 1.4em;
}

.markdown :deep(li) {
  margin: 0.28em 0;
}

/* The text carries the list; the marker only has to say where an item starts. */
.markdown :deep(li::marker) {
  color: var(--faint);
}

/*
 * A sublist and a loose item's paragraphs are inside an item rather than
 * beside it, so they keep the list's own item spacing instead of picking up
 * the full block margin the shared rule above would give them.
 */
.markdown :deep(li > ul),
.markdown :deep(li > ol) {
  margin: 0.15em 0 0;
}

.markdown :deep(li > p) {
  margin: 0.28em 0;
}

.markdown :deep(li:first-child) {
  margin-top: 0;
}

.markdown :deep(li:last-child) {
  margin-bottom: 0;
}

/*
 * A task item is marked by its checkbox, so the list marker would be a second
 * one. The box hangs into the marker column it replaces rather than pushing
 * the item across, which keeps task and plain items on one text column.
 */
.markdown :deep(li:has(> input[type='checkbox'])) {
  list-style: none;
}

/* A checked box is content, not a disabled control: the global fade is not it. */
.markdown :deep(li > input[type='checkbox']) {
  width: 0.95em;
  height: 0.95em;
  margin: 0 0.45em 0 -1.4em;
  opacity: 1;
  vertical-align: -0.1em;
}

.markdown :deep(h1),
.markdown :deep(h2),
.markdown :deep(h3),
.markdown :deep(h4),
.markdown :deep(h5),
.markdown :deep(h6) {
  margin: 1.2em 0 0.55em;
  line-height: 1.25;
}

.markdown :deep(h1) {
  font-size: 1.35em;
}

.markdown :deep(h2) {
  font-size: 1.2em;
}

.markdown :deep(h3) {
  font-size: 1.08em;
}

/*
 * Past the third level the size has run out, so the deeper headings are said
 * with weight and colour instead: browser defaults shrink h5 and h6 below the
 * body text they introduce.
 */
.markdown :deep(h4) {
  font-size: 1em;
  font-weight: 650;
}

.markdown :deep(h5) {
  font-size: 0.95em;
  font-weight: 600;
}

.markdown :deep(h6) {
  color: var(--muted);
  font-size: 0.9em;
  font-weight: 600;
}

/*
 * A heading that opens a section under another heading needs no gap for the
 * text between them, because there is none.
 */
.markdown
  :deep(:where(h1, h2, h3, h4, h5, h6) + :where(h1, h2, h3, h4, h5, h6)) {
  margin-top: 0.6em;
}

/* A rule between sections, not the browser's inset 3D groove. */
.markdown :deep(hr) {
  margin: 1.2em 0;
  border: 0;
  border-top: 1px solid var(--border);
}

/* An image is held to the message column rather than widening it. */
.markdown :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 6px;
}

/* Struck text has been withdrawn, so it recedes rather than only wearing a line. */
.markdown :deep(del) {
  color: var(--muted);
}

/* A key is a thing to press: agent output names shortcuts often enough. */
.markdown :deep(kbd) {
  padding: 0.1em 0.35em;
  border: 1px solid var(--border);
  border-radius: 4px;
  background: var(--sunk);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.85em;
}

.markdown :deep(code) {
  padding: 0.12em 0.3em;
  border-radius: 4px;
  background: var(--sunk);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.88em;
}

.markdown :deep(pre) {
  margin: 0;
  padding: 11px 12px;
  overflow-x: auto;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--sunk);
  tab-size: 2;
}

/*
 * A highlighted block carries both schemes at once, as a custom property per
 * token, and the scheme in use is chosen here rather than by re-rendering the
 * markdown. Shiki's own backgrounds are left unused: the block keeps the sunk
 * surface it has on whichever surface it sits.
 */
.markdown :deep(.shiki),
.markdown :deep(.shiki span) {
  color: var(--tau-code-light);
  font-style: var(--tau-code-light-font-style, normal);
  font-weight: var(--tau-code-light-font-weight, inherit);
  text-decoration: var(--tau-code-light-text-decoration, none);
}

@media (prefers-color-scheme: dark) {
  .markdown :deep(.shiki),
  .markdown :deep(.shiki span) {
    color: var(--tau-code-dark);
    font-style: var(--tau-code-dark-font-style, normal);
    font-weight: var(--tau-code-dark-font-weight, inherit);
    text-decoration: var(--tau-code-dark-text-decoration, none);
  }
}

/*
 * A drawn diagram is ruled like the block its source would have been, but not
 * filled like one: the rule says where the figure begins and ends, and the
 * page it is drawn on is the one the message is on. Its colours are the app's
 * own tokens, passed to the renderer as references, so the scheme it is in is
 * the one around it.
 */
.markdown :deep(.diagram) {
  width: 100%;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: 9px;
}

/*
 * Centred in the message column, and held to it rather than widening it: a
 * diagram wider than the column scales down whole, which is how a picture is
 * read, instead of scrolling sideways the way its source would.
 */
.markdown :deep(.diagram svg) {
  display: block;
  max-width: 100%;
  height: auto;
  margin: 0 auto;
}

/* The button is placed against the wrapper, not the block, so that scrolling a
 * wide block sideways leaves it where it is. */
.markdown :deep(.code-block) {
  position: relative;
}

/*
 * The language the block was fenced with, kept beside the copy button and shown
 * on the same terms: what the block is written in is worth a glance, not a
 * permanent badge over its first line.
 */
.markdown :deep(.code-block[data-tau-lang]::before) {
  content: attr(data-tau-lang);
  position: absolute;
  top: 10px;
  right: 30px;
  opacity: 0;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  line-height: 1;
}

.markdown :deep(.code-block[data-tau-lang]:hover::before) {
  opacity: 0.7;
}

.markdown :deep(.code-copy) {
  display: grid;
  position: absolute;
  top: 5px;
  right: 5px;
  padding: 4px;
  border-radius: 4px;
  opacity: 0;
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  cursor: default;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: none;
  user-select: none;
}

.markdown :deep(.code-block:hover .code-copy) {
  opacity: 0.45;
}

/*
 * Each of these has to carry the block as well: `.code-block:hover .code-copy`
 * above is the more specific selector, so a bare `.code-copy:hover` would lose
 * to it and the icon would never brighten. The copied state is hover-scoped
 * for the same reason it is revealed by hover — the button belongs to the
 * block under the pointer, and leaving takes the acknowledgement with it.
 */
.markdown :deep(.code-block .code-copy:focus-visible),
.markdown :deep(.code-block:hover .code-copy:hover),
.markdown :deep(.code-block:hover .code-copy[data-copied]) {
  outline: 0;
  opacity: 1;
}

.markdown :deep(.code-copy svg) {
  display: block;
  width: 1em;
  height: 1em;
  fill: currentcolor;
}

.markdown :deep(.code-copy .check),
.markdown :deep(.code-copy[data-copied] .copy) {
  display: none;
}

.markdown :deep(.code-copy[data-copied] .check) {
  display: block;
}

.markdown :deep(pre code) {
  padding: 0;
  background: transparent;
}

.markdown :deep(blockquote) {
  padding-left: 14px;
  border-left: 2px solid var(--border);
  color: var(--muted);
}

/*
 * A quote inside a quote is another rule beside the first, so it keeps the
 * spacing of the block it sits in rather than the full block margin.
 */
.markdown :deep(blockquote blockquote) {
  margin: 0.4em 0;
}

/*
 * A table is laid out as a block so that a wider one scrolls within the text
 * rather than stretching whatever holds it; `max-content` is what keeps the
 * columns at their natural width instead of collapsing to the space left.
 * Separate borders are what let the table carry a single outer rule and a
 * radius, which collapsed borders hand to the cells instead.
 */
.markdown :deep(table) {
  display: block;
  width: max-content;
  max-width: 100%;
  margin: 0.7em 0;
  overflow-x: auto;
  border-spacing: 0;
  border-collapse: separate;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 0.94em;
  overflow-wrap: normal;
}

.markdown :deep(th),
.markdown :deep(td) {
  padding: 6px 12px;
}

/*
 * Only the rows are ruled: vertical lines would draw a grid around content
 * the columns already separate.
 */
.markdown :deep(thead th) {
  border-bottom: 1px solid var(--border);
}

.markdown :deep(tbody tr + tr > td) {
  border-top: 1px solid var(--border);
}

.markdown :deep(th) {
  background: var(--sunk);
  font-weight: 600;
}

/* The radius is on the table, so the header has to leave its corners alone. */
.markdown :deep(thead th:first-child) {
  border-top-left-radius: 8px;
}

.markdown :deep(thead th:last-child) {
  border-top-right-radius: 8px;
}

/*
 * A column's alignment is written in markdown and arrives as an `align`
 * attribute, which any rule of ours would outrank: left is the default for a
 * column that did not ask for one, not something imposed on every column.
 */
.markdown :deep(th:not([align])),
.markdown :deep(td:not([align])) {
  text-align: left;
}

/* The hand belongs to links, and these are the only ones: they leave the app. */
.markdown :deep(a) {
  color: var(--link);
  cursor: pointer;
  text-decoration-thickness: 1px;
  text-underline-offset: 2px;
}

/*
 * A path is a link to a file the system opens, so it carries a link's colour
 * but keeps the underline for the pointer rather than wearing one in prose.
 */
.markdown :deep(.file-link) {
  text-decoration: none;
}

.markdown :deep(.file-link:hover),
.markdown :deep(.file-link:focus-visible) {
  outline: 0;
  text-decoration: underline;
}

/* Dragging content out of the transcript is a browser gesture, not an app one. */
.markdown :deep(a),
.markdown :deep(img) {
  -webkit-user-drag: none;
}
</style>
