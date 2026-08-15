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
  isWebUrl,
  parseFileReference,
  renderMarkdown,
  resolveFilePath,
} from '../lib/markdown';

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

async function openFile(path: string, basePath: string): Promise<void> {
  try {
    await openPath(resolveFilePath(basePath, path, await homeDirectory()));
  } catch (error) {
    console.error('Could not open the file in its default app', error);
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

  // A file is opened where it can be, and the window never follows the link out
  // of the app where it cannot.
  event.preventDefault();
  if (props.basePath) await openFile(path, props.basePath);
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter') void activate(event);
}
</script>
