<template>
  <UiConfirmation
    :open="open"
    :title="title"
    :description="sessionDescription"
    :action="action"
    :busy="busy"
    :error="error"
    initial-focus="confirm"
    class-prefix="quit-confirmation"
    @cancel="cancel"
    @confirm="confirm"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import UiConfirmation from './ui/UiConfirmation.vue';

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

const sessionDescription = computed(() => {
  if (props.sessionCount <= 0) return undefined;
  return props.sessionCount === 1
    ? 'One session is still in progress.'
    : `${props.sessionCount} sessions are still in progress.`;
});

function cancel(): void {
  emit('cancel');
}

function confirm(): void {
  emit('confirm');
}
</script>
