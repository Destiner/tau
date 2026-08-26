<template>
  <component
    :is="entry.historyAvailable ? 'button' : 'div'"
    class="compaction-divider"
    :class="{ clickable: entry.historyAvailable, loading: showLoading }"
    :type="entry.historyAvailable ? 'button' : undefined"
    :disabled="entry.historyLoading || undefined"
    :aria-label="entry.historyAvailable ? 'Load earlier messages' : undefined"
    :aria-busy="entry.historyLoading || undefined"
    @click="load"
  >
    <span class="rule"></span>
    <span class="label">{{ label }}</span>
    <span class="rule"></span>
  </component>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';

const props = withDefaults(
  defineProps<{ entry: TranscriptEntry; label?: string }>(),
  { label: 'compacted' },
);
const emit = defineEmits<{ load: [element: HTMLElement] }>();
const showLoading = ref(false);
let loadingTimer: ReturnType<typeof setTimeout> | undefined;

watch(
  () => props.entry.historyLoading,
  (loading) => {
    clearTimeout(loadingTimer);
    showLoading.value = false;
    if (loading) {
      loadingTimer = setTimeout(() => {
        showLoading.value = true;
      }, 180);
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => clearTimeout(loadingTimer));

function load(event: MouseEvent): void {
  if (!props.entry.historyAvailable || props.entry.historyLoading) return;
  emit('load', event.currentTarget as HTMLElement);
}
</script>

<style scoped>
.compaction-divider {
  display: grid;
  grid-template-columns: minmax(12px, 1fr) auto minmax(12px, 1fr);
  align-items: center;
  width: 100%;
  min-height: var(--control-sm);
  margin: 16px 0;
  padding: 0;
  background: transparent;
  color: var(--faint);
  gap: 10px;
}

button.compaction-divider {
  cursor: default;
}

.compaction-divider:focus-visible {
  outline: 0;
}

.rule {
  position: relative;
  height: 1px;
  overflow: hidden;
  background: color-mix(in srgb, var(--border) 72%, transparent);
}

.label {
  min-height: var(--control-sm);
  padding: 4px 2px;
  color: var(--faint);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-tight);
}

.compaction-divider.clickable:hover .label,
.compaction-divider.clickable:focus-visible .label {
  color: var(--muted);
}

.compaction-divider.clickable:hover .rule,
.compaction-divider.clickable:focus-visible .rule {
  background: color-mix(in srgb, var(--muted) 55%, transparent);
}

.compaction-divider.loading .rule::after {
  content: '';
  position: absolute;
  width: 30%;
  animation: loading 850ms ease-in-out infinite;
  opacity: 0.7;
  background: var(--muted);
  inset: 0;
}

@keyframes loading {
  from {
    transform: translateX(-110%);
  }

  to {
    transform: translateX(440%);
  }
}
</style>
