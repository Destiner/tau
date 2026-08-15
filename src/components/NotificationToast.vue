<template>
  <ToastRoot
    :open="open"
    class="ui-toast"
    type="background"
    @update:open="handleOpenChange"
  >
    <div class="extension-notification-copy">
      <div class="extension-notification-header">
        <span class="extension-notification-context">
          {{ notification.projectName }} · {{ notification.sessionName }}
        </span>
        <ToastClose as-child>
          <UiIconButton
            size="xs"
            variant="fill"
            label="Dismiss notification"
          >
            <UiIcon name="cross" />
          </UiIconButton>
        </ToastClose>
      </div>
      <MarkdownText
        class="extension-notification-message"
        :source="notification.message"
        :base-path="notification.workingDirectory"
      />
    </div>
  </ToastRoot>
</template>

<script setup lang="ts">
import { ToastClose, ToastRoot } from 'reka-ui';
import { ref } from 'vue';

import type { ExtensionNotification } from '../composables/state';

import MarkdownText from './ui/MarkdownText.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';

const props = defineProps<{ notification: ExtensionNotification }>();

const emit = defineEmits<{ dismiss: [key: string] }>();

const open = ref(true);

/** Any close — the button, Escape, or a swipe — reports the key for removal. */
function handleOpenChange(next: boolean): void {
  open.value = next;
  if (!next) emit('dismiss', props.notification.key);
}
</script>

<style scoped>
/* The toast root is teleported into the viewport, where the scoped attribute
 * does not reach it, so it is styled with :global on its namespaced class. */
:global(.ui-toast) {
  padding: 7px 8px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--panel-raised);
  box-shadow: 0 10px 30px var(--shadow-soft);
  pointer-events: auto;
}

:global(.ui-toast[data-state='open']) {
  animation: toast-in 160ms ease-out;
}

:global(.ui-toast[data-state='closed']) {
  animation: toast-out 160ms ease-in forwards;
}

@keyframes toast-in {
  from {
    transform: translateY(8px);
    opacity: 0;
  }

  to {
    transform: translateY(0);
    opacity: 1;
  }
}

@keyframes toast-out {
  from {
    transform: translateY(0);
    opacity: 1;
  }

  to {
    transform: translateY(8px);
    opacity: 0;
  }
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
.extension-notification-header :deep(> button) {
  margin-left: auto;
}

.extension-notification-message {
  color: var(--text);
  font-size: 12px;
  line-height: 1.45;
}
</style>
