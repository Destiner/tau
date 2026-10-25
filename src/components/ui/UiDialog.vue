<template>
  <DialogRoot
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="ui-dialog-overlay ui-modal-overlay" />
      <DialogContent
        class="ui-dialog-content ui-modal"
        :data-modal-size="width === 'md' ? 'wide' : 'standard'"
        :aria-busy="busy || undefined"
        v-bind="$attrs"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <div class="ui-dialog-scroll">
          <header class="ui-dialog-head">
            <DialogTitle class="ui-dialog-title ui-modal-title">{{
              title
            }}</DialogTitle>
            <DialogDescription
              v-if="description"
              class="ui-dialog-desc ui-modal-description"
              >{{ description }}</DialogDescription
            >
          </header>
          <div
            v-if="$slots.default"
            class="ui-dialog-body"
          >
            <slot />
          </div>
        </div>
        <footer
          v-if="$slots.footer"
          class="ui-dialog-foot ui-modal-actions"
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

defineOptions({ inheritAttrs: false });

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
  z-index: 20;
}

:global(.ui-dialog-content) {
  z-index: 21;
}

:global(.ui-dialog-scroll) {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  overflow: auto;
}

:global(.ui-dialog-head) {
  display: flex;
  flex: none;
  flex-direction: column;
  padding: var(--modal-gutter);
  gap: 8px;
}

:global(.ui-dialog-desc) {
  margin: 0;
}

:global(.ui-dialog-body) {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 0 var(--modal-gutter) var(--modal-gutter);
  overflow: auto;
  gap: 8px;
}
</style>
