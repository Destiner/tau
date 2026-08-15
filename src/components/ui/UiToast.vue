<template>
  <ToastProvider
    :duration="duration"
    :swipe-direction="swipeDirection"
    :label="label"
  >
    <ToastViewport class="ui-toast-viewport" />
    <slot />
  </ToastProvider>
</template>

<script setup lang="ts">
import { ToastProvider, ToastViewport } from 'reka-ui';

withDefaults(
  defineProps<{
    /** Milliseconds a toast stays before auto-closing; Infinity keeps it until dismissed. */
    duration?: number;
    /** Pointer swipe that closes a toast. */
    swipeDirection?: 'right' | 'left' | 'up' | 'down';
    /** Announced to assistive tech to tie a toast to its trigger. */
    label?: string;
  }>(),
  {
    duration: 5000,
    swipeDirection: 'right',
    label: 'Notification',
  },
);
</script>

<style scoped>
/* The toasts teleport into this viewport, which reka renders and the scoped
 * attribute does not reach, so it is styled with :global. */
:global(.ui-toast-viewport) {
  display: flex;
  position: fixed;
  z-index: 30;
  right: 12px;
  bottom: 12px;
  flex-direction: column;
  width: min(360px, calc(100vw - 24px));
  outline: none;
  pointer-events: none;
  gap: 7px;
}
</style>
