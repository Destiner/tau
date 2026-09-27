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
        <div
          v-if="$slots.default"
          class="ui-dialog-body"
        >
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
    title: string;

    description?: string;

    width?: 'sm' | 'md';
    busy?: boolean;

    dismissible?: boolean;

    returnFocus?: () => HTMLElement | undefined;
  }>(),
  {
    description: undefined,
    width: 'sm',
    dismissible: true,
    returnFocus: undefined,
  },
);

function handleOpenChange(next: boolean): void {
  if (!next && (props.busy || !props.dismissible)) return;
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
:global(.ui-dialog-overlay) {
  position: fixed;
  z-index: 20;
  background: var(--scrim);
  inset: 0;
}

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
