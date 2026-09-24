<template>
  <div class="markdown-shell">
    <MarkdownContent
      v-bind="$attrs"
      :source="source"
      :inline="inline"
      :base-path="basePath"
      :copy-paths="copyPaths"
      :remote-project-path="remoteProjectPath"
      preview-files
      @file-preview="openFileViewer"
    />
    <FileViewer
      v-if="fileViewer"
      v-bind="fileViewer"
      :return-focus="fileViewerReturnFocus"
      @close="closeFileViewer"
      @replace="replaceFileViewer"
    />
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';

import type { FilePreviewDescriptor } from '../../lib/file-preview';
import { invokeTraced } from '../../lib/telemetry';

import FileViewer from './FileViewer.vue';
import MarkdownContent from './MarkdownContent.vue';

defineProps<{
  source: string;
  inline?: boolean;
  basePath?: string;
  /** Remote file paths preview rather than opening on this machine. */
  copyPaths?: boolean;
  remoteProjectPath?: string;
}>();

defineOptions({ inheritAttrs: false });

const fileViewer = ref<FilePreviewDescriptor | null>(null);
let fileViewerTrigger: HTMLElement | undefined;

function openFileViewer(
  preview: FilePreviewDescriptor,
  trigger: HTMLElement,
): void {
  const previous = fileViewer.value;
  fileViewerTrigger = trigger;
  fileViewer.value = preview;
  if (previous) void releaseFilePreview(previous.id);
}

function replaceFileViewer(preview: FilePreviewDescriptor): void {
  const previous = fileViewer.value;
  fileViewer.value = preview;
  if (previous) void releaseFilePreview(previous.id);
}

function fileViewerReturnFocus(): HTMLElement | undefined {
  return fileViewerTrigger?.isConnected ? fileViewerTrigger : undefined;
}

async function releaseFilePreview(id: string): Promise<void> {
  try {
    await invokeTraced('release_file_preview', { id });
  } catch {
    console.error('Could not release the file preview');
  }
}

function closeFileViewer(): void {
  const preview = fileViewer.value;
  fileViewer.value = null;
  if (preview) void releaseFilePreview(preview.id);
}

onBeforeUnmount(closeFileViewer);
</script>

<style scoped>
/* Preserve the rendered surface as the element seen by transcript callers. */
.markdown-shell {
  display: contents;
}
</style>
