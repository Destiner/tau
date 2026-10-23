<template>
  <DialogRoot
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogOverlay
        class="ui-confirmation-overlay ui-modal-overlay"
        :class="`${classPrefix}-overlay`"
      />
      <DialogContent
        ref="content"
        class="ui-confirmation ui-modal"
        :class="classPrefix"
        data-modal-size="compact"
        role="alertdialog"
        :aria-busy="busy || undefined"
        @keydown.capture="handleKeydown"
        @open-auto-focus="handleOpenAutoFocus"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <div
          class="ui-confirmation-copy"
          :class="`${classPrefix}-copy`"
        >
          <DialogTitle
            class="ui-confirmation-title ui-modal-title"
            :class="`${classPrefix}-title`"
          >
            {{ title }}
          </DialogTitle>
          <DialogDescription
            v-if="description"
            class="ui-confirmation-description ui-modal-description"
            :class="`${classPrefix}-description`"
          >
            {{ description }}
          </DialogDescription>
          <p
            v-if="error"
            class="ui-confirmation-error"
            :class="`${classPrefix}-error`"
            role="alert"
          >
            {{ error }}
          </p>
        </div>
        <footer
          class="ui-confirmation-actions ui-modal-actions"
          :class="`${classPrefix}-actions`"
        >
          <UiButton
            ref="cancelButton"
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
            {{ busyLabelVisible && busyAction ? busyAction : action }}
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
import { nextTick, ref, watch } from 'vue';

import UiButton from './UiButton.vue';
import registerConfirmation from './confirmation-stack';

const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    action: string;
    description?: string;
    busy?: boolean;
    busyAction?: string;
    error?: string;
    initialFocus?: 'cancel' | 'confirm';
    returnFocus?: () => HTMLElement | undefined;
    classPrefix?: string;
  }>(),
  {
    description: '',
    busy: false,
    busyAction: '',
    error: '',
    initialFocus: 'cancel',
    returnFocus: undefined,
    classPrefix: 'ui-confirmation',
  },
);

const emit = defineEmits<{
  cancel: [];
  confirm: [];
}>();

const busyLabelVisible = ref(false);
watch(
  () => props.busy,
  (busy, _, onCleanup) => {
    busyLabelVisible.value = false;
    if (!busy) return;
    const timer = setTimeout(() => {
      busyLabelVisible.value = true;
    }, 200);
    onCleanup(() => clearTimeout(timer));
  },
);

const content = ref<InstanceType<typeof DialogContent>>();
const cancelButton = ref<InstanceType<typeof UiButton>>();
const confirmButton = ref<InstanceType<typeof UiButton>>();

watch(
  () => [props.open, content.value] as const,
  ([open, element], _, onCleanup) => {
    if (!open || !element) return;
    onCleanup(registerConfirmation(element.$el as HTMLElement, cancel));
  },
  { flush: 'post' },
);

function handleOpenChange(next: boolean): void {
  if (!next) cancel();
}

function handleOpenAutoFocus(event: Event): void {
  event.preventDefault();
  void nextTick(() => {
    const button =
      props.initialFocus === 'confirm'
        ? confirmButton.value
        : cancelButton.value;
    button?.button?.focus({ preventScroll: true });
  });
}

function handleCloseAutoFocus(event: Event): void {
  const otherModal = anotherModal();
  if (otherModal) {
    event.preventDefault();
    const target =
      otherModal.querySelector<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), textarea:not(:disabled)',
      ) ?? otherModal;
    target.focus({ preventScroll: true });
    return;
  }
  const target = props.returnFocus?.();
  if (target) {
    event.preventDefault();
    target.focus({ preventScroll: true });
    return;
  }

  if (props.returnFocus) event.preventDefault();
}

function anotherModal(): HTMLElement | undefined {
  return [
    ...document.querySelectorAll<HTMLElement>(
      '[role="dialog"], [role="alertdialog"]',
    ),
  ]
    .filter((dialog) => {
      if (dialog === content.value?.$el) return false;
      const style = window.getComputedStyle(dialog);
      return style.display !== 'none' && style.visibility !== 'hidden';
    })
    .at(-1);
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.isComposing) return;
  if (event.key === 'Tab') {
    event.preventDefault();
    event.stopImmediatePropagation();
    if (props.busy) return;
    const cancelControl = cancelButton.value?.button;
    const confirmControl = confirmButton.value?.button;
    const target =
      document.activeElement === cancelControl ? confirmControl : cancelControl;
    target?.focus();
    return;
  }

  if (
    event.key === 'Enter' &&
    (event.metaKey || event.ctrlKey) &&
    !props.busy
  ) {
    event.preventDefault();
    event.stopImmediatePropagation();
    confirm();
  }
}

function cancel(): void {
  if (!props.busy) emit('cancel');
}

function confirm(): void {
  if (!props.busy) emit('confirm');
}
</script>

<style scoped>
:global(.ui-confirmation-overlay) {
  z-index: 30;
}

:global(.ui-confirmation) {
  z-index: 31;
}

:global(.ui-confirmation-copy) {
  min-height: 0;
  padding: var(--modal-gutter);
  overflow: auto;
}

:global(.ui-confirmation-description),
:global(.ui-confirmation-error) {
  margin: 8px 0 0;
}

:global(.ui-confirmation-error) {
  color: var(--danger);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}
</style>
