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
import { openPath, openUrl } from '@tauri-apps/plugin-opener';
import { computed } from 'vue';

import {
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

let homePath: Promise<string> | null = null;

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

async function activate(event: Event): Promise<void> {
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
  if (event.key === 'Enter') void activate(event);
}
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
.markdown :deep(pre),
.markdown :deep(blockquote) {
  margin: 0.7em 0;
}

.markdown :deep(h1),
.markdown :deep(h2),
.markdown :deep(h3) {
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

.markdown :deep(code) {
  padding: 0.12em 0.3em;
  border-radius: 4px;
  background: var(--sunk);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.88em;
}

.markdown :deep(pre) {
  padding: 11px 12px;
  overflow-x: auto;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--sunk);
}

.markdown :deep(pre code) {
  padding: 0;
  background: transparent;
}

.markdown :deep(blockquote) {
  padding-left: 12px;
  border-left: 2px solid var(--border);
  color: var(--muted);
}

/*
 * A table is laid out as a block so that a wider one scrolls within the text
 * rather than stretching whatever holds it; `max-content` is what keeps the
 * columns at their natural width instead of collapsing to the space left.
 */
.markdown :deep(table) {
  display: block;
  width: max-content;
  max-width: 100%;
  margin: 0.7em 0;
  overflow-x: auto;
  border-collapse: collapse;
  font-size: 0.94em;
  overflow-wrap: normal;
}

.markdown :deep(th),
.markdown :deep(td) {
  padding: 5px 9px;
  border: 1px solid var(--border);
  text-align: left;
}

.markdown :deep(th) {
  background: var(--sunk);
  font-weight: 600;
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
