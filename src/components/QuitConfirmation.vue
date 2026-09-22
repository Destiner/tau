<template>
  <DialogRoot
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="quit-confirmation-overlay" />
      <DialogContent
        class="quit-confirmation"
        role="alertdialog"
        :aria-busy="busy || undefined"
        @open-auto-focus="handleOpenAutoFocus"
      >
        <div class="quit-confirmation-copy">
          <DialogTitle class="quit-confirmation-title">{{ title }}</DialogTitle>
          <DialogDescription
            v-if="sessionCount > 0"
            class="quit-confirmation-description"
          >
            {{ sessionDescription }}
          </DialogDescription>
          <p
            v-if="error"
            class="quit-confirmation-error"
            role="alert"
          >
            {{ error }}
          </p>
        </div>
        <footer class="quit-confirmation-actions">
          <UiButton
            size="md"
            :disabled="busy"
            @click="cancel"
          >
            Cancel
          </UiButton>
          <UiButton
            ref="confirmButton"
            size="md"
            variant="primary"
            :disabled="busy"
            @click="confirm"
          >
            {{ action }}
          </UiButton>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<script setup lang="ts">
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from 'reka-ui';
import { computed, nextTick, ref, watchEffect } from 'vue';

import UiButton from './ui/UiButton.vue';

const props = withDefaults(
  defineProps<{
    open: boolean;
    sessionCount: number;
    busy?: boolean;
    error?: string;
    title?: string;
    action?: string;
  }>(),
  { busy: false, error: '', title: 'Quit Tau?', action: 'Quit' },
);

const emit = defineEmits<{
  cancel: [];
  confirm: [];
}>();

const confirmButton = ref<InstanceType<typeof UiButton>>();
const sessionDescription = computed(() =>
  props.sessionCount === 1
    ? 'One session is still in progress.'
    : `${props.sessionCount} sessions are still in progress.`,
);

function handleOpenChange(next: boolean): void {
  if (!next && !props.busy) cancel();
}

function handleOpenAutoFocus(event: Event): void {
  event.preventDefault();
  void nextTick(() => confirmButton.value?.button?.focus());
}

// Reka's window listener runs too late to suppress app and native Escape handling.
watchEffect((onCleanup) => {
  if (!props.open) return;
  document.addEventListener('keydown', handleEscapeKeyDown, true);
  onCleanup(() =>
    document.removeEventListener('keydown', handleEscapeKeyDown, true),
  );
});

function handleEscapeKeyDown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return;
  event.preventDefault();
  event.stopImmediatePropagation();
  cancel();
}

function cancel(): void {
  if (!props.busy) emit('cancel');
}

function confirm(): void {
  if (!props.busy) emit('confirm');
}
</script>

<style scoped>
/* The dialog is portaled to body, outside this component's scoped subtree. */
:global(.quit-confirmation-overlay) {
  position: fixed;
  z-index: 30;
  background: color-mix(in srgb, var(--scrim) 72%, transparent);
  inset: 0;
}

:global(.quit-confirmation) {
  display: flex;
  position: fixed;
  z-index: 31;
  top: 112px;
  left: 50%;
  flex-direction: column;
  width: min(348px, calc(100% - 36px));
  overflow: hidden;
  transform: translateX(-50%);
  border: 1px solid color-mix(in srgb, var(--border) 75%, transparent);
  border-radius: var(--radius-lg);
  outline: 0;
  background: color-mix(in srgb, var(--panel-raised) 96%, transparent);
  box-shadow: 0 18px 42px var(--shadow-strong);
  backdrop-filter: blur(22px);
}

:global(.quit-confirmation-copy) {
  padding: 18px 20px 14px;
}

:global(.quit-confirmation-title) {
  margin: 0;
  color: var(--text);
  font-size: var(--text-md);
  font-weight: 500;
  line-height: var(--leading-tight);
}

:global(.quit-confirmation-description),
:global(.quit-confirmation-error) {
  margin: 6px 0 0;
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

:global(.quit-confirmation-description) {
  color: var(--muted);
}

:global(.quit-confirmation-error) {
  color: var(--danger);
}

:global(.quit-confirmation-actions) {
  display: flex;
  justify-content: flex-end;
  padding: 0 16px 14px;
  gap: 8px;
}
</style>
