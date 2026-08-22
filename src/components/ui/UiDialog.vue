<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="ui-dialog-overlay" />
      <DialogContent
        class="ui-dialog-content"
        :class="`width-${width}`"
        :aria-busy="busy || undefined"
      >
        <DialogTitle class="ui-dialog-title">{{ title }}</DialogTitle>
        <slot />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<script setup lang="ts">
import {
  DialogContent,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from 'reka-ui';

const open = defineModel<boolean>('open', { default: false });

withDefaults(
  defineProps<{
    /** Accessible name, announced on open; the visible panel has no heading. */
    title: string;
    /** Panel width: 440px for a connection, 480px for a directory browser. */
    width?: 'sm' | 'md';
    busy?: boolean;
  }>(),
  { width: 'sm' },
);
</script>

<style scoped>
/* reka portals the overlay and panel to the body, where the scoped
 * attribute does not reach them, so they are styled with :global on
 * their namespaced classes. */
:global(.ui-dialog-overlay) {
  position: fixed;
  z-index: 20;
  background: var(--scrim);
  inset: 0;
}

/* The panel is a sibling of the overlay, not a child, so it positions
 * itself: top-centered, clear of the title bar. */
:global(.ui-dialog-content) {
  position: fixed;
  z-index: 21;
  top: 68px;
  left: 50%;
  width: min(440px, calc(100% - 36px));
  padding: 8px;
  transform: translateX(-50%);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  box-shadow: 0 14px 40px var(--shadow-strong);
}

:global(.ui-dialog-content.width-md) {
  width: min(480px, calc(100% - 36px));
}

/* The title is announced, not shown: the panel explains itself. */
:global(.ui-dialog-title) {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip-path: inset(50%);
  border: 0;
  white-space: nowrap;
}
</style>
