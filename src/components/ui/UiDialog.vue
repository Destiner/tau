<template>
  <DialogRoot
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="ui-dialog-overlay" />
      <DialogContent
        class="ui-dialog-content"
        :class="`width-${width}`"
        :aria-busy="busy || undefined"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <header class="ui-dialog-head">
          <DialogTitle class="ui-dialog-title">{{ title }}</DialogTitle>
          <DialogDescription
            v-if="description"
            class="ui-dialog-desc"
            >{{ description }}</DialogDescription
          >
        </header>
        <div class="ui-dialog-body">
          <slot />
        </div>
        <footer
          v-if="$slots.footer"
          class="ui-dialog-foot"
        >
          <slot name="footer" />
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

const open = defineModel<boolean>('open', { default: false });

const props = withDefaults(
  defineProps<{
    /** Shown as the panel's heading, and announced on open. */
    title: string;
    /** One line under the title, when the panel needs to explain itself. */
    description?: string;
    /** Panel width: 440px for a connection, 480px for a directory browser. */
    width?: 'sm' | 'md';
    busy?: boolean;
    /** Persistent control that receives focus after this dialog closes. */
    returnFocus?: () => HTMLElement | undefined;
  }>(),
  { description: undefined, width: 'sm', returnFocus: undefined },
);

function handleOpenChange(next: boolean): void {
  if (!next && props.busy) return;
  open.value = next;
}

function handleCloseAutoFocus(event: Event): void {
  const target = props.returnFocus?.();
  if (!target) return;
  event.preventDefault();
  target.focus({ preventScroll: true });
}
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

/*
 * The panel is a sibling of the overlay, not a child, so it positions itself:
 * top-centered, clear of the title bar. It does not animate in — an animation
 * that touches `transform` drops the centring translate for its first frame,
 * which reads as the panel sliding in from the right.
 */
:global(.ui-dialog-content) {
  display: flex;
  position: fixed;
  z-index: 21;
  top: 68px;
  left: 50%;
  flex-direction: column;
  width: min(440px, calc(100% - 36px));
  max-height: calc(100vh - 120px);
  overflow: hidden;
  transform: translateX(-50%);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  box-shadow: 0 14px 40px var(--shadow-strong);
}

:global(.ui-dialog-content.width-md) {
  width: min(480px, calc(100% - 36px));
}

/*
 * Head, body and foot are parts, so a dialog stops re-rolling its own padding.
 * Every margin is set explicitly: the title renders as an h2 and the
 * description as a p, whose default margins are what spread the header out.
 */
:global(.ui-dialog-head) {
  display: flex;
  flex-direction: column;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  gap: 3px;
}

:global(.ui-dialog-title) {
  margin: 0;
  color: var(--text);
  font-size: var(--text-md);
  font-weight: 500;
  line-height: var(--leading-tight);
}

:global(.ui-dialog-desc) {
  margin: 0;
  color: var(--muted);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

:global(.ui-dialog-body) {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 12px;
  overflow: auto;
  gap: 8px;
}

:global(.ui-dialog-foot) {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
  gap: 6px;
}
</style>
