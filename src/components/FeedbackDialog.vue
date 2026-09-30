<template>
  <UiDialog
    :open="open"
    :title="incident?.title ?? 'Operation Failed'"
    :description="incident?.message"
    :busy="busy"
    :dismissible="dismissible"
    :return-focus="returnFocus"
    @update:open="handleOpenChange"
  >
    <template #footer>
      <div ref="footerElement">
        <UiButton
          v-if="incident?.action"
          variant="primary"
          :disabled="busy"
          :title="
            primaryShortcut ? `${actionLabel} (${primaryShortcut})` : undefined
          "
          @click="requestAction"
          >{{ actionLabel }}</UiButton
        >
        <UiButton
          v-if="dismissible"
          :disabled="busy"
          @click="requestClose"
          >Close</UiButton
        >
      </div>
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import type { FeedbackIncident } from '../composables/state';
import {
  appCommandDispatchKey,
  appCommandLookupKey,
} from '../lib/app-commands/binding';
import {
  formatShortcut,
  shortcutMatches,
  type ShortcutPlatform,
} from '../lib/app-commands/shortcut';

import UiButton from './ui/UiButton.vue';
import UiDialog from './ui/UiDialog.vue';

const props = defineProps<{
  open: boolean;
  incident?: FeedbackIncident;
  busy?: boolean;
  returnFocus?: () => HTMLElement | undefined;
}>();

const emit = defineEmits<{
  action: [];
  close: [];
}>();

const commandDispatch = inject(appCommandDispatchKey, undefined);
const commandHint = inject(appCommandLookupKey, undefined);
const platform: ShortcutPlatform = /Mac|iPhone|iPad/.test(navigator.platform)
  ? 'mac'
  : 'non-mac';
const primaryShortcut = computed(() => {
  const shortcut = commandHint?.('feedback.action')?.shortcut;
  return shortcut ? formatShortcut(shortcut, platform) : undefined;
});
const footerElement = ref<HTMLElement>();
let dialogContent: HTMLElement | undefined;

const dismissible = computed(
  () =>
    props.incident?.action !== 'initialize' &&
    props.incident?.action !== 'reload',
);
const actionLabel = computed(() => {
  switch (props.incident?.action) {
    case 'initialize':
      return 'Try Again';
    case 'reconnect':
      return 'Reconnect';
    case 'reload':
      return 'Reload Tau';
    default:
      return '';
  }
});

watch(
  () => props.open,
  (open) => {
    removeDialogKeydown();
    if (!open) return;
    void nextTick(() => {
      if (!props.open) return;
      dialogContent =
        footerElement.value?.closest('.ui-dialog-content') ?? undefined;
      dialogContent?.addEventListener('keydown', handleKeydown);
    });
  },
  { flush: 'post', immediate: true },
);
onBeforeUnmount(removeDialogKeydown);

function removeDialogKeydown(): void {
  dialogContent?.removeEventListener('keydown', handleKeydown);
  dialogContent = undefined;
}

function handleOpenChange(open: boolean): void {
  if (!open) emit('close');
}

function requestAction(): void {
  if (commandDispatch) commandDispatch('feedback.action');
  else emit('action');
}

function requestClose(): void {
  emit('close');
}

function handleKeydown(event: KeyboardEvent): void {
  if (event.isComposing) return;

  const shortcut = commandHint?.('feedback.action')?.shortcut ?? 'Mod+Enter';
  if (shortcutMatches(event, shortcut, platform) && props.incident?.action) {
    event.preventDefault();
    event.stopPropagation();
    if (!props.busy) requestAction();
  }
}
</script>
