<template>
  <UiToast
    :duration="Infinity"
    label="Extension notification"
  >
    <NotificationToast
      v-for="notification in notifications"
      :key="notification.key"
      :notification="notification"
      @dismiss="handleDismiss"
    />
  </UiToast>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import type { ExtensionNotification } from '../composables/state';

import NotificationToast from './NotificationToast.vue';
import UiToast from './ui/UiToast.vue';

defineProps<{ notifications: ExtensionNotification[] }>();

const emit = defineEmits<{ dismiss: [key: string] }>();

/** Time the exit animation takes; a dismissal waits it out before removal. */
const EXIT_MS = 200;
const closing = ref(new Set<string>());

function handleDismiss(key: string): void {
  if (closing.value.has(key)) return;
  closing.value.add(key);
  window.setTimeout(() => {
    closing.value.delete(key);
    emit('dismiss', key);
  }, EXIT_MS);
}
</script>
