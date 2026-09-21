<template>
  <div
    class="reconnect-status"
    role="status"
  >
    <UiStatusDot tone="working" />
    <span>{{ message }}</span>
    <UiButton
      size="sm"
      variant="ghost"
      :disabled="disabled || busy"
      @click="requestReconnect"
      >{{ busy ? 'Reconnecting…' : 'Reconnect' }}</UiButton
    >
  </div>
</template>

<script setup lang="ts">
import UiButton from './ui/UiButton.vue';
import UiStatusDot from './ui/UiStatusDot.vue';

defineProps<{
  message: string;
  busy?: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{ reconnect: [] }>();

function requestReconnect(): void {
  emit('reconnect');
}
</script>

<style scoped>
.reconnect-status {
  --status-dot-nudge: 0;

  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 40px;
  gap: 8px;
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.reconnect-status :deep(.ui-status-dot) {
  align-self: center;
}
</style>
