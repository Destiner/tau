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
      <UiButton
        v-if="incident?.action"
        variant="primary"
        :disabled="busy"
        @click="requestAction"
        >{{ actionLabel }}</UiButton
      >
      <UiButton
        v-if="dismissible"
        :disabled="busy"
        @click="requestClose"
        >Close</UiButton
      >
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { FeedbackIncident } from '../composables/state';

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

function handleOpenChange(open: boolean): void {
  if (!open) emit('close');
}

function requestAction(): void {
  emit('action');
}

function requestClose(): void {
  emit('close');
}
</script>
