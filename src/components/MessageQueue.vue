<template>
  <section
    class="message-queue"
    aria-label="Pending messages"
    @keydown.escape="handleEscape"
  >
    <UiTooltip
      v-if="count"
      text="Clear All"
    >
      <UiIconButton
        size="sm"
        variant="fill"
        label="Clear All"
        :disabled="busy"
        @click="clear"
        ><UiIcon
          name="cross"
          style="width: 0.8em; height: 0.8em"
      /></UiIconButton>
    </UiTooltip>
    <div
      class="queue-rail"
      aria-label="Pending messages; scroll horizontally for more"
    >
      <button
        v-for="(text, index) in queue.steering"
        :key="`s-${index}`"
        class="queue-chip"
        type="button"
        :aria-label="`Steering: ${text}`"
        @mouseenter="(event) => scheduleMessageShow(event, 'steering', text)"
        @mouseleave="hideMessage"
        @focus="(event) => showMessage(event, 'steering', text)"
        @blur="scheduleMessageClose"
      >
        <span class="queue-snippet">{{ text }}</span>
      </button>
      <span
        v-if="queue.steering.length && queue.followUp.length"
        class="queue-divider"
        aria-hidden="true"
      ></span>
      <button
        v-for="(text, index) in queue.followUp"
        :key="`f-${index}`"
        class="queue-chip"
        type="button"
        :aria-label="`Follow Up ${index + 1}: ${text}`"
        @mouseenter="
          (event) => scheduleMessageShow(event, 'followUp', text, index + 1)
        "
        @mouseleave="hideMessage"
        @focus="(event) => showMessage(event, 'followUp', text, index + 1)"
        @blur="scheduleMessageClose"
      >
        <span
          class="queue-ordinal"
          aria-hidden="true"
          >{{ index + 1 }}</span
        ><span class="queue-snippet">{{ text }}</span>
      </button>
    </div>
    <span
      v-if="feedback || busy"
      class="queue-feedback"
      role="status"
      >{{ feedback || 'Clearing…' }}</span
    >
    <UiIconButton
      v-if="feedback && !busy"
      size="sm"
      variant="fade"
      label="Dismiss queue status"
      @click="dismiss"
      ><UiIcon name="cross"
    /></UiIconButton>
    <button
      v-if="failedDrafts.length"
      type="button"
      class="recover"
      @click="showFailed"
    >
      Review Unsent ({{ failedDrafts.length }})
    </button>
    <Teleport to="body">
      <div
        v-if="messagePreview"
        ref="previewElement"
        class="message-preview"
        :style="previewStyle"
        role="tooltip"
        @mouseenter="cancelMessageClose"
        @mouseleave="scheduleMessageClose"
        @focusin="cancelMessageClose"
        @focusout="scheduleMessageClose"
        @keydown.escape="handleEscape"
      >
        <strong>{{
          messagePreview.kind === 'steering'
            ? 'Steering'
            : `Follow Up • ${messagePreview.order}`
        }}</strong>
        <MarkdownText :source="messagePreview.text" />
      </div>
    </Teleport>
    <div
      v-if="details"
      class="queue-details"
      role="region"
      aria-label="Message details"
    >
      <strong>Unsent messages</strong>
      <div
        v-for="(text, index) in failedDrafts"
        :key="index"
        class="detail-row"
      >
        <span>{{ text }}</span
        ><UiButton
          size="sm"
          variant="ghost"
          :disabled="!canRecover"
          @click="() => emit('recover', index)"
          >Restore Draft</UiButton
        >
      </div>
      <p
        v-if="!canRecover"
        class="recovery-hint"
      >
        Clear the composer to restore a draft.
      </p>
      <UiButton
        size="sm"
        variant="ghost"
        @click="closeDetails"
        >Close</UiButton
      >
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import type { QueueSnapshot } from '../lib/pi/queue';

import MarkdownText from './ui/MarkdownText.vue';
import UiButton from './ui/UiButton.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiTooltip from './ui/UiTooltip.vue';

const props = defineProps<{
  queue: QueueSnapshot;
  feedback: string;
  busy: boolean;
  failedDrafts: string[];
  canRecover: boolean;
}>();
const emit = defineEmits<{
  clear: [];
  recover: [index: number];
  dismiss: [];
}>();
const details = ref<'failed' | null>(null);
const messagePreview = ref<{
  kind: 'steering' | 'followUp';
  text: string;
  order?: number;
} | null>(null);
const previewStyle = ref<Record<string, string>>({});
const previewElement = ref<HTMLElement | null>(null);
let previewVersion = 0;
let previewTrigger: HTMLElement | undefined;
let suppressPreviewFocus = false;
let messageShowTimer: ReturnType<typeof setTimeout> | undefined;
let messageCloseTimer: ReturnType<typeof setTimeout> | undefined;
let detailsTrigger: HTMLButtonElement | undefined;
const count = computed(
  () => props.queue.steering.length + props.queue.followUp.length,
);
watch(
  () => props.queue,
  () => {
    cancelMessageShow();
    cancelMessageClose();
    previewVersion++;
    messagePreview.value = null;
  },
);
function clear(): void {
  emit('clear');
}
function dismiss(): void {
  emit('dismiss');
}
function scheduleMessageShow(
  event: Event,
  kind: 'steering' | 'followUp',
  text: string,
  order?: number,
): void {
  cancelMessageShow();
  const target = event.currentTarget as HTMLElement;
  messageShowTimer = setTimeout(() => {
    if (target.isConnected) showMessageAt(target, kind, text, order);
  }, 180);
}
function showMessage(
  event: Event,
  kind: 'steering' | 'followUp',
  text: string,
  order?: number,
): void {
  if (suppressPreviewFocus) {
    suppressPreviewFocus = false;
    return;
  }
  cancelMessageShow();
  showMessageAt(event.currentTarget as HTMLElement, kind, text, order);
}
function showMessageAt(
  target: HTMLElement,
  kind: 'steering' | 'followUp',
  text: string,
  order?: number,
): void {
  cancelMessageClose();
  previewTrigger = target;
  const rect = target.getBoundingClientRect();
  const version = ++previewVersion;
  previewStyle.value = {
    left: `${Math.max(8, rect.left)}px`,
    top: `${rect.top - 5}px`,
    visibility: 'hidden',
  };
  messagePreview.value = { kind, text, order };
  void nextTick(() => {
    if (previewVersion !== version || !previewElement.value) return;
    const { width, height } = previewElement.value.getBoundingClientRect();
    const above = rect.top - 13;
    const below = window.innerHeight - rect.bottom - 13;
    const openBelow = height > above && below > above;
    previewStyle.value = {
      left: `${Math.max(8, Math.min(rect.left, window.innerWidth - width - 8))}px`,
      top: `${openBelow ? rect.bottom + 5 : rect.top - 5}px`,
      maxHeight: `${Math.max(24, Math.min(340, window.innerHeight * 0.45, openBelow ? below : above))}px`,
      '--preview-shift': openBelow ? '0px' : '-100%',
    };
  });
}
function hideMessage(): void {
  cancelMessageShow();
  scheduleMessageClose();
}
function cancelMessageShow(): void {
  if (messageShowTimer) clearTimeout(messageShowTimer);
  messageShowTimer = undefined;
}
function scheduleMessageClose(): void {
  cancelMessageClose();
  messageCloseTimer = setTimeout(() => {
    previewVersion++;
    messagePreview.value = null;
  }, 60);
}
function cancelMessageClose(): void {
  if (messageCloseTimer) clearTimeout(messageCloseTimer);
  messageCloseTimer = undefined;
}
onBeforeUnmount(() => {
  cancelMessageShow();
  cancelMessageClose();
});
function showFailed(): void {
  showDetails('failed');
}
function showDetails(kind: 'failed'): void {
  if (document.activeElement instanceof HTMLButtonElement)
    detailsTrigger = document.activeElement;
  details.value = details.value === kind ? null : kind;
}
function handleEscape(event: KeyboardEvent): void {
  if (messagePreview.value) {
    event.stopPropagation();
    event.preventDefault();
    cancelMessageClose();
    previewVersion++;
    messagePreview.value = null;
    if (
      previewTrigger?.isConnected &&
      document.activeElement !== previewTrigger
    ) {
      suppressPreviewFocus = true;
      previewTrigger.focus();
    }
    return;
  }
  if (!details.value) return;
  event.stopPropagation();
  event.preventDefault();
  closeDetails();
}
function closeDetails(): void {
  if (!details.value) return;
  details.value = null;
  void nextTick(() => detailsTrigger?.isConnected && detailsTrigger.focus());
}
</script>

<style scoped>
.message-queue {
  display: flex;
  position: relative;
  align-items: center;
  min-width: 0;
  padding: 4px;
  font-size: var(--text-sm);
  gap: 4px;
}

.queue-rail {
  display: flex;
  flex: 0 1 auto;
  width: max-content;
  min-width: 0;
  max-width: 100%;
  overflow-x: auto;
  gap: 4px;
  overscroll-behavior-inline: contain;
  scrollbar-width: thin;
}

.queue-chip {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 4px;
  max-width: min(230px, 75%);
  height: var(--control-sm);
  padding: 0 5px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text);
  text-align: left;
}

.queue-divider {
  flex: none;
  align-self: center;
  height: 14px;
  border-left: 1px solid var(--border);
}

.queue-chip .queue-snippet {
  min-width: 0;
  overflow: hidden;
  color: var(--muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.queue-chip .queue-ordinal {
  display: grid;
  flex: none;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 4px;
  place-items: center;
  background: var(--hover);
  color: var(--muted);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
}

/* stylelint-disable no-descending-specificity -- hover rules follow controls */
.queue-chip:hover,
.recover:hover {
  background: var(--hover);
}

.queue-chip:focus-visible,
.recover:focus-visible {
  outline: 2px solid var(--accent);
}

.queue-feedback {
  max-width: min(220px, 35vw);
  overflow: hidden;
  color: var(--muted);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.recover {
  flex: none;
  padding: 2px 4px;
  background: transparent;
  color: var(--muted);
  font-size: var(--text-xs);
}

/* stylelint-enable no-descending-specificity */
.message-preview {
  position: fixed;
  z-index: 100;
  width: fit-content;
  max-width: min(500px, calc(100vw - 16px));
  max-height: min(45vh, 340px);
  padding: 9px;
  overflow: auto;
  transform: translateY(var(--preview-shift, -100%));
  animation: message-preview-appear 130ms ease-out;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--panel-raised);
  box-shadow: 0 5px 16px var(--shadow-soft);
  color: var(--text);
  font-size: var(--text-sm);
}

.message-preview > strong {
  display: block;
  margin-bottom: 5px;
  font-size: var(--text-xs);
}

@keyframes message-preview-appear {
  from {
    transform: translateY(calc(var(--preview-shift, -100%) + 3px));
    opacity: 0;
  }

  to {
    transform: translateY(var(--preview-shift, -100%));
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .message-preview {
    animation: none;
  }
}

.queue-details {
  position: absolute;
  z-index: 4;
  right: 4px;
  bottom: 100%;
  left: 4px;
  max-height: min(36vh, 260px);
  padding: 8px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--panel-raised);
  box-shadow: 0 4px 14px var(--shadow-soft);
}

.queue-details strong {
  font-size: var(--text-xs);
}

.detail-row {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 4px 6px;
  color: var(--muted);
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.detail-row span {
  min-width: 0;
}

.recovery-hint {
  margin: 4px 6px;
  color: var(--muted);
  font-size: var(--text-xs);
}
</style>
