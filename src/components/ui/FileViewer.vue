<template>
  <DialogRoot
    open
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogContent
        class="file-viewer"
        :aria-busy="loading || undefined"
        @close-auto-focus="handleCloseAutoFocus"
        @open-auto-focus="handleOpenAutoFocus"
      >
        <header class="file-viewer-header">
          <div class="file-viewer-location">
            <DialogTitle class="file-viewer-filename">{{
              filename
            }}</DialogTitle>
            <span class="file-viewer-directory">{{ directory }}</span>
          </div>
        </header>

        <UiIconButton
          ref="closeButton"
          class="file-viewer-close"
          label="Close Preview"
          size="lg"
          variant="fade"
          @click="close"
        >
          <UiIcon name="cross" />
        </UiIconButton>

        <div
          ref="content"
          class="file-viewer-content"
        >
          <template v-if="previewType.kind === 'text' && text !== null">
            <!-- eslint-disable vue/no-v-html -- highlightCode returns only Shiki-generated markup -->
            <div
              v-if="highlighted"
              class="file-viewer-code"
              v-html="highlighted"
            ></div>
            <!-- eslint-enable vue/no-v-html -->
            <pre
              v-else
              class="file-viewer-plain"
            ><code>{{ text }}</code></pre>
          </template>

          <img
            v-else-if="previewType.kind === 'image'"
            class="file-viewer-image"
            :src="assetUrl"
            :alt="filename"
            @load="finishLoading"
            @error="failLoading"
          />

          <video
            v-else-if="previewType.kind === 'video'"
            class="file-viewer-media"
            :src="assetUrl"
            controls
            preload="metadata"
            @loadedmetadata="finishLoading"
            @error="failLoading"
          >
            Preview unavailable.
          </video>

          <audio
            v-else-if="previewType.kind === 'audio'"
            class="file-viewer-audio"
            :src="assetUrl"
            controls
            preload="metadata"
            @loadedmetadata="finishLoading"
            @error="failLoading"
          >
            Preview unavailable.
          </audio>

          <object
            v-else-if="
              previewType.kind === 'pdf' || previewType.kind === 'object'
            "
            class="file-viewer-object"
            :data="assetUrl"
            :type="previewType.mediaType"
            @load="finishLoading"
            @error="failLoading"
          >
            <p class="file-viewer-fallback">Preview unavailable.</p>
          </object>
        </div>

        <div
          v-if="showLoading && loading"
          class="file-viewer-state"
          role="status"
        >
          Loading preview
        </div>
        <div
          v-else-if="failure"
          class="file-viewer-state"
          role="alert"
        >
          Couldn’t preview this file.
        </div>
        <div
          v-if="truncated && !failure"
          class="file-viewer-limit"
          role="status"
        >
          Showing first 512 KiB
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<script setup lang="ts">
import { DialogContent, DialogPortal, DialogRoot, DialogTitle } from 'reka-ui';
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import {
  classifyFilePreview,
  fetchTextPreview,
  filePreviewAssetUrl,
  type FilePreviewDescriptor,
} from '../../lib/file-preview';
import highlightCode from '../../lib/highlight';

import UiIcon from './UiIcon.vue';
import UiIconButton from './UiIconButton.vue';

const props = defineProps<
  FilePreviewDescriptor & {
    /** Persistent control that receives focus after the viewer closes. */
    returnFocus?: () => HTMLElement | undefined;
  }
>();

const emit = defineEmits<{ close: [] }>();

const closeButton = ref<InstanceType<typeof UiIconButton>>();
const content = ref<HTMLElement>();
const text = ref<string | null>(null);
const truncated = ref(false);
const loading = ref(false);
const showLoading = ref(false);
const failure = ref(false);
const previewType = computed(() => classifyFilePreview(props.filename));
const assetUrl = computed(() => filePreviewAssetUrl(props.assetPath));
const highlighted = computed(() => {
  const source = text.value;
  const language = previewType.value.language;
  return source !== null && language ? highlightCode(source, language) : null;
});

let request: AbortController | undefined;
let loadingDelay = 0;
let loadingDeadline = 0;

function clearLoadingTimers(): void {
  window.clearTimeout(loadingDelay);
  window.clearTimeout(loadingDeadline);
}

function beginLoading(): void {
  clearLoadingTimers();
  loading.value = true;
  showLoading.value = false;
  failure.value = false;
  loadingDelay = window.setTimeout(() => {
    showLoading.value = true;
  }, 200);
  loadingDeadline = window.setTimeout(failLoading, 15_000);
}

function finishLoading(): void {
  clearLoadingTimers();
  loading.value = false;
  showLoading.value = false;
}

function failLoading(): void {
  clearLoadingTimers();
  loading.value = false;
  showLoading.value = false;
  failure.value = true;
}

async function loadPreview(): Promise<void> {
  request?.abort();
  request = new AbortController();
  text.value = null;
  truncated.value = false;

  beginLoading();
  if (previewType.value.kind !== 'text') return;

  const activeRequest = request;
  try {
    const preview = await fetchTextPreview(
      assetUrl.value,
      props.byteLength,
      activeRequest.signal,
    );
    if (activeRequest !== request) return;
    text.value = preview.text;
    truncated.value = preview.truncated;
    finishLoading();
  } catch {
    if (activeRequest !== request || activeRequest.signal.aborted) return;
    failLoading();
  }
}

function close(): void {
  emit('close');
}

function handleOpenChange(open: boolean): void {
  if (!open) emit('close');
}

function handleKeydownCapture(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopImmediatePropagation();
    emit('close');
    return;
  }
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const activeElement = document.activeElement;
  if (
    activeElement !== closeButton.value?.button &&
    activeElement !== document.body
  ) {
    return;
  }

  const scroller = content.value;
  if (!scroller) return;
  const page = Math.max(scroller.clientHeight * 0.85, 1);
  const distance =
    event.key === 'ArrowDown'
      ? { top: 40 }
      : event.key === 'ArrowUp'
        ? { top: -40 }
        : event.key === 'ArrowRight'
          ? { left: 40 }
          : event.key === 'ArrowLeft'
            ? { left: -40 }
            : event.key === 'PageDown'
              ? { top: page }
              : event.key === 'PageUp'
                ? { top: -page }
                : null;
  if (distance) {
    event.preventDefault();
    scroller.scrollBy(distance);
    return;
  }
  if (event.key === 'Home' || event.key === 'End') {
    event.preventDefault();
    scroller.scrollTo({
      top: event.key === 'Home' ? 0 : scroller.scrollHeight,
    });
  }
}

function handleOpenAutoFocus(event: Event): void {
  event.preventDefault();
  void nextTick(() => closeButton.value?.button?.focus());
}

function handleCloseAutoFocus(event: Event): void {
  const target = props.returnFocus?.();
  if (!target) return;
  event.preventDefault();
  target.focus({ preventScroll: true });
}

watch(
  () => [props.id, props.assetPath, props.filename, props.byteLength],
  () => void loadPreview(),
  { immediate: true },
);

onMounted(() => {
  document.addEventListener('keydown', handleKeydownCapture, true);
});

onBeforeUnmount(() => {
  document.removeEventListener('keydown', handleKeydownCapture, true);
  request?.abort();
  clearLoadingTimers();
});
</script>

<style scoped>
/* Over every panel and dialog, matching the fullscreen diagram viewer. */
:global(.file-viewer) {
  position: fixed;
  z-index: 30;
  overflow: hidden;
  background: var(--preview-canvas);
  color: var(--text);
  inset: 0;
}

:global(.file-viewer-header) {
  display: grid;
  position: absolute;
  z-index: 2;
  top: 0;
  right: 0;
  left: 0;
  height: 46px;
  background: linear-gradient(
    to bottom,
    color-mix(in srgb, var(--preview-canvas) 96%, transparent),
    color-mix(in srgb, var(--preview-canvas) 70%, transparent),
    transparent
  );
  pointer-events: none;
  place-items: center;
}

:global(.file-viewer-location) {
  display: flex;
  align-items: baseline;
  max-width: min(70vw, 620px);
  gap: 7px;
  font-size: var(--text-sm);
  white-space: nowrap;
}

:global(.file-viewer-filename),
:global(.file-viewer-directory) {
  overflow: hidden;
  text-overflow: ellipsis;
}

:global(.file-viewer-filename) {
  flex: none;
  margin: 0;
  color: var(--text);
  font-size: inherit;
  font-weight: 400;
}

:global(.file-viewer-directory) {
  color: var(--muted);
}

:global(.file-viewer-close) {
  position: absolute;
  z-index: 3;
  top: 9px;
  right: 12px;
}

:global(.file-viewer-content) {
  position: absolute;
  overflow: auto;
  inset: 46px 0 0;
}

:global(.file-viewer-code),
:global(.file-viewer-plain) {
  min-width: min-content;
  min-height: 100%;
  margin: 0;
  padding: 20px 40px 40px;
  tab-size: 2;
  cursor: text;
  user-select: text;
}

:global(.file-viewer-code pre) {
  min-width: min-content;
  min-height: 100%;
  margin: 0;
  background: transparent !important;
}

:global(.file-viewer-code code),
:global(.file-viewer-plain code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-md);
  line-height: 1.65;
}

:global(.file-viewer-code .shiki),
:global(.file-viewer-code .shiki span) {
  color: var(--tau-code-light);
  font-style: var(--tau-code-light-font-style, normal);
  font-weight: var(--tau-code-light-font-weight, inherit);
  text-decoration: var(--tau-code-light-text-decoration, none);
}

:global(.file-viewer-image),
:global(.file-viewer-media) {
  display: block;
  width: 100%;
  height: 100%;
  margin: auto;
  object-fit: contain;
}

:global(.file-viewer-audio) {
  display: block;
  width: min(480px, calc(100% - 40px));
  margin: 40px auto;
}

:global(.file-viewer-object) {
  display: block;
  width: 100%;
  height: 100%;
  border: 0;
}

:global(.file-viewer-fallback),
:global(.file-viewer-state) {
  position: absolute;
  top: 50%;
  left: 50%;
  margin: 0;
  transform: translate(-50%, -50%);
  color: var(--muted);
  font-size: var(--text-sm);
}

:global(.file-viewer-state) {
  z-index: 4;
}

:global(.file-viewer-limit) {
  position: absolute;
  z-index: 3;
  right: 12px;
  bottom: 10px;
  padding: 3px 7px;
  border-radius: var(--radius-sm);
  background: var(--preview-canvas);
  box-shadow: 0 0 0 1px var(--border);
  color: var(--muted);
  font-size: var(--text-xs);
}

@media (prefers-color-scheme: dark) {
  :global(.file-viewer-code .shiki),
  :global(.file-viewer-code .shiki span) {
    color: var(--tau-code-dark);
    font-style: var(--tau-code-dark-font-style, normal);
    font-weight: var(--tau-code-dark-font-weight, inherit);
    text-decoration: var(--tau-code-dark-text-decoration, none);
  }
}

@media (width <= 720px) {
  :global(.file-viewer-code),
  :global(.file-viewer-plain) {
    padding-right: 20px;
    padding-left: 20px;
  }
}
</style>
