<template>
  <div class="markdown-shell">
    <!-- eslint-disable vue/no-v-html -- renderMarkdown sanitizes with DOMPurify -->
    <UiContextMenu
      :items="contextMenuItems"
      :min-width="128"
    >
      <div
        v-bind="$attrs"
        class="markdown"
        @click="activate"
        @contextmenu.stop="prepareContextMenu"
        @keydown="handleKeydown"
        @pointerdown="beginPointerActivation"
        @pointermove="trackPointerActivation"
        @pointercancel="cancelPointerActivation"
        v-html="rendered"
      ></div>
    </UiContextMenu>
    <Teleport to="body">
      <span
        v-if="pathFeedback"
        class="path-feedback"
        :class="pathFeedback.kind"
        :style="pathFeedback.style"
        role="status"
        >{{ pathFeedback.text }}</span
      >
    </Teleport>
    <!-- eslint-enable vue/no-v-html -->
    <DiagramViewer
      v-if="viewer"
      :svg="viewer.svg"
      :x="viewer.x"
      :y="viewer.y"
      :width="viewer.width"
      :height="viewer.height"
      :return-focus="viewerReturnFocus"
      @close="closeViewer"
    />
  </div>
</template>

<script setup lang="ts">
import { homeDir } from '@tauri-apps/api/path';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { openPath, openUrl } from '@tauri-apps/plugin-opener';
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import {
  CODE_COPY_ATTRIBUTE,
  DIAGRAM_EXPAND_ATTRIBUTE,
  FILE_PATH_ATTRIBUTE,
  isPathOpenGesture,
  isWebUrl,
  parseMarkdownFileDestination,
  renderMarkdown,
  resolveFilePath,
} from '../../lib/markdown';
import {
  createPathPreviewCoordinator,
  pathPreviewCoordinatorKey,
} from '../../lib/path-preview-coordinator';
import { piOwnerArgs } from '../../lib/pi/ownership';
import {
  remotePreviewErrorCopy,
  remotePreviewErrorKind,
} from '../../lib/remote-preview-errors';
import { invokeTraced } from '../../lib/telemetry';

import DiagramViewer from './DiagramViewer.vue';
import UiContextMenu from './UiContextMenu.vue';
import type { UiMenuItem } from './UiMenu.vue';

const props = defineProps<{
  source: string;
  inline?: boolean;
  basePath?: string;
  /** Remote file paths preview rather than opening on this machine. */
  copyPaths?: boolean;
  remoteProjectPath?: string;
}>();

defineOptions({ inheritAttrs: false });

const rendered = computed(() =>
  renderMarkdown(props.source, {
    ...(props.inline ? { inline: true } : {}),
    ...(props.basePath ? { basePath: props.basePath } : {}),
    ...(props.copyPaths ? { copyPaths: true } : {}),
  }),
);

/** How long a copied block keeps saying so before the icon returns. */
const COPIED_FEEDBACK_MS = 1_200;

let homePath: Promise<string> | null = null;
let copiedTimer: ReturnType<typeof setTimeout> | undefined;
let copiedElement: HTMLElement | undefined;
let feedbackAnchor: HTMLElement | undefined;
let feedbackRevision = 0;
let mounted = true;
const previewCoordinator = inject(
  pathPreviewCoordinatorKey,
  createPathPreviewCoordinator(),
);
const previewOrigin = Symbol('markdown-path-origin');
let activeRemoteRequest: string | undefined;
let pointerActivation:
  { link: HTMLElement; x: number; y: number; moved: boolean } | undefined;
interface PathFeedback {
  text: string;
  kind: 'success' | 'error' | 'loading';
  style: Record<string, string>;
}
const pathFeedback = ref<PathFeedback | null>(null);

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
  if (written !== null) return written || null;

  const href = link.getAttribute('href');
  return href ? parseMarkdownFileDestination(href) : null;
}

function actionIsCurrent(revision: number): boolean {
  return mounted && revision === feedbackRevision;
}

async function openLocalPath(
  path: string,
  basePath: string,
  anchor: HTMLElement,
): Promise<void> {
  const revision = ++feedbackRevision;
  try {
    const home = path.startsWith('~/') ? await homeDirectory() : undefined;
    if (!actionIsCurrent(revision)) return;
    await openPath(resolveFilePath(basePath, path, home));
  } catch (error) {
    console.error('Could not open the path', error);
    if (actionIsCurrent(revision))
      showPathFeedback(anchor, 'Could not open path. Try again.', 'error');
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

interface DiagramView {
  svg: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

const viewer = ref<DiagramView | null>(null);
let viewerTrigger: HTMLElement | undefined;

function expandButtonAt(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  const button = target.closest(`button[${DIAGRAM_EXPAND_ATTRIBUTE}]`);
  return button instanceof HTMLElement ? button : null;
}

/** The natural coordinate bounds the renderer named on the drawing. */
function diagramBounds(svg: SVGSVGElement): {
  x: number;
  y: number;
  width: number;
  height: number;
} {
  const box = svg.viewBox.baseVal;
  if (box.width > 0 && box.height > 0) {
    return { x: box.x, y: box.y, width: box.width, height: box.height };
  }

  const width = Number.parseFloat(svg.getAttribute('width') ?? '');
  const height = Number.parseFloat(svg.getAttribute('height') ?? '');
  return { x: 0, y: 0, width, height };
}

function openDiagram(button: HTMLElement): void {
  const svg = button.parentElement?.querySelector('svg');
  if (!svg) return;
  const bounds = diagramBounds(svg);
  if (!(bounds.width > 0 && bounds.height > 0)) return;
  viewerTrigger = button;
  viewer.value = { svg: svg.outerHTML, ...bounds };
}

/** The viewer hands focus back to the button that opened it, if it is still here. */
function viewerReturnFocus(): HTMLElement | undefined {
  return viewerTrigger?.isConnected ? viewerTrigger : undefined;
}

function closeViewer(): void {
  viewer.value = null;
}

/** Marks the copied code button, so its icon reports that the copy landed. */
function showCopied(element: HTMLElement): void {
  clearCopied();
  copiedElement = element;
  copiedElement.dataset.copied = 'true';
  copiedTimer = setTimeout(clearCopied, COPIED_FEEDBACK_MS);
}

function showPathFeedback(
  anchor: HTMLElement,
  text: string,
  kind: PathFeedback['kind'],
): void {
  clearCopied();
  const rect = anchor.getBoundingClientRect();
  if (!anchor.isConnected) return;
  feedbackAnchor = anchor;
  pathFeedback.value = {
    text,
    kind,
    style: feedbackPosition(rect),
  };
  if (kind !== 'loading')
    copiedTimer = setTimeout(clearCopied, COPIED_FEEDBACK_MS);
}

function clearCopied(): void {
  clearTimeout(copiedTimer);
  copiedTimer = undefined;
  delete copiedElement?.dataset.copied;
  copiedElement = undefined;
  feedbackAnchor = undefined;
  pathFeedback.value = null;
}

function feedbackPosition(rect: DOMRect): Record<string, string> {
  return {
    left: `${Math.min(Math.max(8, rect.left), Math.max(8, window.innerWidth - 180))}px`,
    top: `${Math.min(Math.max(8, rect.bottom + 6), Math.max(8, window.innerHeight - 36))}px`,
  };
}

function updateFeedbackPosition(): void {
  if (!pathFeedback.value || !feedbackAnchor?.isConnected) {
    clearCopied();
    return;
  }
  pathFeedback.value.style = feedbackPosition(
    feedbackAnchor.getBoundingClientRect(),
  );
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

async function copyPath(path: string, anchor: HTMLElement): Promise<void> {
  const revision = ++feedbackRevision;
  try {
    await writeText(path);
  } catch (error) {
    console.error('Could not copy the path', error);
    if (revision === feedbackRevision)
      showPathFeedback(anchor, 'Could not copy path. Try again.', 'error');
    return;
  }
  if (revision === feedbackRevision)
    showPathFeedback(anchor, 'Path Copied', 'success');
}

/** A remote home is unknown, so a tilde path cannot truthfully be made full. */
function canCopyFullPath(path: string): boolean {
  return (
    path.startsWith('/') ||
    (Boolean(props.basePath) && (!props.copyPaths || !path.startsWith('~/')))
  );
}

async function copyFullPath(path: string, anchor: HTMLElement): Promise<void> {
  const revision = ++feedbackRevision;
  try {
    const home = path.startsWith('~/') ? await homeDirectory() : undefined;
    if (!actionIsCurrent(revision)) return;
    await writeText(resolveFilePath(props.basePath ?? '', path, home));
  } catch (error) {
    console.error('Could not copy the full path', error);
    if (actionIsCurrent(revision))
      showPathFeedback(anchor, 'Could not copy path. Try again.', 'error');
    return;
  }
  if (actionIsCurrent(revision))
    showPathFeedback(anchor, 'Path Copied', 'success');
}

interface ContextCopyTarget {
  kind: 'url' | 'path';
  value: string;
}

let contextCopyTarget: ContextCopyTarget | null = null;
let contextCopyAnchor: HTMLElement | null = null;

/** Keeps the webview menu suppressed unless the pointer names something copyable. */
function prepareContextMenu(event: MouseEvent): void {
  const link = linkAt(event.target);
  const href = link?.getAttribute('href');
  contextCopyAnchor = link;
  if (href && isWebUrl(href)) {
    contextCopyTarget = { kind: 'url', value: href };
    return;
  }

  const path = link ? filePath(link) : null;
  if (path) {
    contextCopyTarget = { kind: 'path', value: path };
    return;
  }

  contextCopyTarget = null;
  contextCopyAnchor = null;
  event.preventDefault();
}

async function copyUrl(url: string): Promise<void> {
  try {
    await writeText(url);
  } catch (error) {
    console.error('Could not copy the URL', error);
  }
}

function contextMenuItems(): UiMenuItem[] {
  const target = contextCopyTarget;
  const anchor = contextCopyAnchor;
  if (!target || !anchor) return [];
  if (target.kind === 'url') {
    return [{ label: 'Copy URL', run: () => void copyUrl(target.value) }];
  }

  const items: UiMenuItem[] = [
    { label: 'Copy Path', run: () => void copyPath(target.value, anchor) },
  ];
  if (canCopyFullPath(target.value)) {
    items.push({
      label: 'Copy Full Path',
      run: () => void copyFullPath(target.value, anchor),
    });
  }
  return items;
}

let previewAvailable: Promise<boolean> | undefined;

async function activateRemotePath(
  path: string,
  anchor: HTMLElement,
): Promise<void> {
  if (!props.remoteProjectPath) {
    await copyPath(path, anchor);
    return;
  }
  const revision = ++feedbackRevision;
  const requestId = crypto.randomUUID();
  const projectPath = props.remoteProjectPath;
  const cancel = (): void => {
    if (activeRemoteRequest === requestId) activeRemoteRequest = undefined;
    if (revision === feedbackRevision) {
      feedbackRevision += 1;
      clearCopied();
    }
    void invokeTraced('cancel_remote_path', {
      ...piOwnerArgs(),
      requestId,
    }).catch(() => undefined);
  };
  if (
    previewCoordinator.start(
      previewOrigin,
      `${projectPath}\0${path}`,
      requestId,
      cancel,
    ) === 'duplicate'
  )
    return;
  activeRemoteRequest = requestId;
  previewAvailable ??= invokeTraced<boolean>('remote_preview_available');
  let loading: ReturnType<typeof setTimeout> | undefined;
  try {
    if (!(await previewAvailable)) {
      if (actionIsCurrent(revision)) await copyPath(path, anchor);
      return;
    }
    if (!actionIsCurrent(revision)) return;
    loading = setTimeout(() => {
      if (actionIsCurrent(revision))
        showPathFeedback(anchor, 'Preparing preview…', 'loading');
    }, 200);
    const prepared = await invokeTraced<
      { kind: 'directory' } | { kind: 'file'; token: string }
    >('prepare_remote_path', {
      ...piOwnerArgs(),
      projectPath,
      requestId,
      path,
    });
    clearTimeout(loading);
    if (!actionIsCurrent(revision)) return;
    if (prepared.kind === 'directory') {
      await copyPath(path, anchor);
      return;
    }
    await invokeTraced('show_remote_preview', {
      ...piOwnerArgs(),
      requestId,
      token: prepared.token,
    });
    if (actionIsCurrent(revision)) clearCopied();
  } catch (error) {
    clearTimeout(loading);
    if (actionIsCurrent(revision)) {
      const kind = remotePreviewErrorKind(error);
      if (kind !== 'superseded')
        showPathFeedback(anchor, remotePreviewErrorCopy(kind), 'error');
    }
  } finally {
    previewCoordinator.finish(requestId);
    if (activeRemoteRequest === requestId) activeRemoteRequest = undefined;
  }
}

async function activate(event: Event): Promise<void> {
  const copy = copyButtonAt(event.target);
  if (copy) {
    await copyCodeBlock(copy);
    return;
  }

  const expand = expandButtonAt(event.target);
  if (expand) {
    openDiagram(expand);
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

  // No non-web Markdown destination may navigate Tau's own webview, including
  // malformed or unsupported destinations.
  event.preventDefault();
  const path = filePath(link);
  if (!path || !isPathOpenGesture(event, window.navigator.platform)) return;
  if (
    event instanceof MouseEvent &&
    (pointerActivation?.moved ||
      (window.getSelection()?.isCollapsed === false &&
        Boolean(window.getSelection()?.toString())))
  )
    return;
  if (props.copyPaths) {
    await activateRemotePath(path, link);
    return;
  }
  if (props.basePath) await openLocalPath(path, props.basePath, link);
}

function handleKeydown(event: KeyboardEvent): void {
  // A copy or expand button is a button: the browser turns Enter and Space
  // into a click, so handling those here as well would act twice.
  if (copyButtonAt(event.target) || expandButtonAt(event.target)) return;
  const remotePathButton =
    props.copyPaths &&
    event.key === ' ' &&
    linkAt(event.target)?.hasAttribute(FILE_PATH_ATTRIBUTE);
  if (event.key === 'Enter' || remotePathButton) void activate(event);
}

function beginPointerActivation(event: PointerEvent): void {
  const link = linkAt(event.target);
  pointerActivation =
    event.button === 0 && link
      ? { link, x: event.clientX, y: event.clientY, moved: false }
      : undefined;
}

function trackPointerActivation(event: PointerEvent): void {
  if (!pointerActivation) return;
  if (
    Math.abs(event.clientX - pointerActivation.x) > 4 ||
    Math.abs(event.clientY - pointerActivation.y) > 4
  )
    pointerActivation.moved = true;
}

function cancelPointerActivation(): void {
  pointerActivation = undefined;
}

function cancelPendingAction(): void {
  feedbackRevision += 1;
  clearCopied();
  previewCoordinator.cancelOrigin(previewOrigin);
}

watch(
  () => [
    props.source,
    props.basePath,
    props.remoteProjectPath,
    props.copyPaths,
  ],
  cancelPendingAction,
);
onMounted(() => {
  window.addEventListener('resize', updateFeedbackPosition);
  document.addEventListener('scroll', updateFeedbackPosition, true);
});
onBeforeUnmount(() => {
  mounted = false;
  window.removeEventListener('resize', updateFeedbackPosition);
  document.removeEventListener('scroll', updateFeedbackPosition, true);
  cancelPendingAction();
});
</script>

<style scoped>
/* Keep the component's status region out of layout while forwarding callers'
 * classes to the markdown surface they have always styled. */
.markdown-shell {
  display: contents;
}

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

/* Prose keeps a readable measure without taking width away from figures and
 * structured content that benefit from the whole message column. */
.markdown
  :deep(
    > :where(
      p:not(:has(> img:only-child)),
      ul,
      ol,
      blockquote,
      h1,
      h2,
      h3,
      h4,
      h5,
      h6,
      hr
    )
  ) {
  max-width: 80ch;
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

.markdown :deep(strong) {
  font-weight: 600;
}

.markdown :deep(:where(h1, h2, h3, h4, h5, h6) strong) {
  font-weight: inherit;
}

.markdown :deep(h1),
.markdown :deep(h2),
.markdown :deep(h3),
.markdown :deep(h4),
.markdown :deep(h5),
.markdown :deep(h6) {
  margin: 1.2em 0 0.55em;
  font-weight: 650;
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
  line-height: 1.5;
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
  position: relative;
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
 * The expand button keeps the copy button's terms: revealed by pointing at
 * the figure, bright under the pointer. It backs itself with the canvas the
 * diagram is drawn on, so it stays legible over the figure's own strokes.
 */
.markdown :deep(.diagram-expand) {
  display: grid;
  position: absolute;
  top: 8px;
  right: 8px;
  padding: 4px;
  border-radius: 4px;
  opacity: 0;
  background: var(--canvas);
  color: var(--muted);
  font-size: 13px;
  cursor: default;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: none;
  user-select: none;
}

.markdown :deep(.diagram:hover .diagram-expand) {
  opacity: 0.45;
}

/* Each carries the figure as well, for the same reason the copy button does:
 * the hover reveal above is the more specific selector. */
.markdown :deep(.diagram .diagram-expand:focus-visible),
.markdown :deep(.diagram:hover .diagram-expand:hover) {
  outline: 0;
  opacity: 1;
}

.markdown :deep(.diagram-expand svg) {
  display: block;
  width: 1em;
  height: 1em;
  fill: currentcolor;
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
  font-size: var(--text-xs);
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

.path-feedback {
  position: fixed;
  z-index: 1000;
  max-width: 240px;
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--raised);
  box-shadow: var(--shadow-sm);
  color: var(--text);
  font-size: var(--text-xs);
  line-height: 1.3;
  white-space: nowrap;
  pointer-events: none;
}

.path-feedback.error {
  color: var(--danger);
}

/* Dragging content out of the transcript is a browser gesture, not an app one. */
.markdown :deep(a),
.markdown :deep(img) {
  -webkit-user-drag: none;
}
</style>
