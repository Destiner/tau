<template>
  <div class="preview">
    <header class="preview-controls">
      <span class="preview-name">Queue UX · Mock Pi</span>
      <span class="preview-instructions"
        >Send any message to start work. Queue messages while working.</span
      >
      <span class="preview-status">{{ status }}</span>
      <UiButton
        size="sm"
        variant="secondary"
        :disabled="busy || status === 'Idle'"
        @click="advance"
        >Advance Pi</UiButton
      >
      <UiButton
        size="sm"
        variant="ghost"
        @click="reset"
        >Reset</UiButton
      >
    </header>
    <App class="real-app" />
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';

import App from '../App.vue';
import UiButton from '../components/ui/UiButton.vue';

import { queuePreviewControls } from './queue-preview-controls';

const status = ref('Idle');
const busy = ref(false);
let timer: ReturnType<typeof setInterval> | undefined;
onMounted(() => {
  timer = setInterval(() => {
    status.value = queuePreviewControls.value?.status() ?? 'Idle';
  }, 150);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});
async function advance(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    await queuePreviewControls.value?.advance();
  } finally {
    busy.value = false;
    status.value = queuePreviewControls.value?.status() ?? 'Idle';
  }
}
function reset(): void {
  window.location.reload();
}
</script>

<style scoped>
.preview {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

.preview-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
  color: var(--muted);
  font-size: var(--text-xs);
}

.preview-name {
  flex: none;
  color: var(--text);
  font-weight: 600;
}

.preview-instructions {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.preview-status {
  flex: none;
  white-space: nowrap;
}

.real-app {
  min-height: 0;
}

@media (width <= 600px) {
  .preview-instructions {
    display: none;
  }
}
</style>
