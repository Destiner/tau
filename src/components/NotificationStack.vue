<template>
  <div
    class="extension-notification-stack"
    aria-live="polite"
  >
    <div
      v-for="notification in notifications"
      :key="notification.key"
      class="extension-notification"
      :class="notification.type"
    >
      <div class="extension-notification-copy">
        <div class="extension-notification-header">
          <span class="extension-notification-context">
            {{ notification.projectName }} · {{ notification.sessionName }}
          </span>
          <UiIconButton
            size="xs"
            variant="fill"
            label="Dismiss notification"
            @click="() => dismiss(notification.key)"
          >
            <UiIcon name="cross" />
          </UiIconButton>
        </div>
        <MarkdownText
          class="extension-notification-message"
          :source="notification.message"
          :base-path="notification.workingDirectory"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { ExtensionNotification } from '../composables/state';

import MarkdownText from './ui/MarkdownText.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';

defineProps<{
  notifications: ExtensionNotification[];
}>();

const emit = defineEmits<{ dismiss: [key: string] }>();

function dismiss(key: string): void {
  emit('dismiss', key);
}
</script>

<style scoped>
.extension-notification-stack {
  display: flex;
  position: fixed;
  z-index: 30;
  right: 12px;
  bottom: 12px;
  flex-direction: column;
  width: min(360px, calc(100vw - 24px));
  pointer-events: none;
  gap: 7px;
}

.extension-notification {
  padding: 7px 8px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--panel-raised);
  box-shadow: 0 10px 30px var(--shadow-soft);
  pointer-events: auto;
}

.extension-notification-copy {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 3px;
}

.extension-notification-header {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 8px;
}

.extension-notification-context {
  overflow: hidden;
  color: var(--muted);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The dismiss button is a UiIconButton; only its push to the right is layout. */
:deep(.extension-notification-header > button) {
  margin-left: auto;
}

.extension-notification-message {
  color: var(--text);
  font-size: 12px;
  line-height: 1.45;
}
</style>
