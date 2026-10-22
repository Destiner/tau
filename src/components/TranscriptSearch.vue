<template>
  <div
    class="transcript-search"
    role="search"
    aria-label="Find in Transcript"
  >
    <div class="search-controls">
      <input
        ref="input"
        :value="query"
        aria-label="Find in Transcript"
        placeholder="Find in transcript"
        spellcheck="false"
        @input="handleInput"
        @keydown="handleKeydown"
      />
      <span
        class="match-count"
        aria-live="polite"
        >{{ status }}</span
      >
      <UiIconButton
        label="Previous Match"
        :disabled="pending || !total"
        @pointerdown.prevent
        @click="previous"
        ><UiIcon name="chevron-up"
      /></UiIconButton>
      <UiIconButton
        label="Next Match"
        :disabled="pending || !total"
        @pointerdown.prevent
        @click="next"
        ><UiIcon name="chevron-down"
      /></UiIconButton>
      <UiIconButton
        label="Close Find"
        @pointerdown.prevent
        @click="close"
        ><UiIcon name="cross"
      /></UiIconButton>
    </div>
    <span
      v-if="loadedOnly"
      class="search-scope"
      >Loaded messages only</span
    >
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';

const props = defineProps<{
  query: string;
  total: number;
  current: number;
  busy: boolean;
  pending: boolean;
  loadedOnly: boolean;
}>();
const emit = defineEmits<{
  query: [value: string];
  navigate: [direction: number];
  close: [];
}>();
const input = ref<HTMLInputElement>();
const status = computed(() =>
  props.busy
    ? 'Indexing…'
    : props.pending || !props.query.trim()
      ? ''
      : props.total
        ? `${props.current} / ${props.total}`
        : 'No matches',
);
function handleInput(event: Event): void {
  emit('query', (event.target as HTMLInputElement).value);
}
function previous(): void {
  emit('navigate', -1);
}
function next(): void {
  emit('navigate', 1);
}
function close(): void {
  emit('close');
}
function handleKeydown(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return;
  if (
    event.key === 'Enter' &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (!props.pending) emit('navigate', event.shiftKey ? -1 : 1);
  }
}
function focus(select = false): void {
  input.value?.focus({ preventScroll: true });
  if (select) input.value?.select();
}
defineExpose({ focus });
</script>

<style scoped>
.transcript-search {
  position: absolute;
  z-index: 2;
  top: 38px;
  right: 12px;
  width: min(324px, calc(100% - 24px));
}

.search-controls {
  display: flex;
  align-items: center;
  padding: 5px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--panel-raised);
  box-shadow: 0 3px 12px var(--shadow-soft);
  gap: 2px;
}

.search-controls input {
  flex: 1;
  width: 0;
  min-width: 35px;
  height: 23px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--canvas);
  font-size: 12px;
}

.search-controls input:focus {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}

.search-controls :deep(svg) {
  width: 14px;
  height: 14px;
}

.match-count {
  min-width: 58px;
  color: var(--muted);
  font-size: 11px;
  text-align: center;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.search-scope {
  padding: 3px 6px;
  float: right;
  border-radius: 0 0 var(--radius-sm) var(--radius-sm);
  background: var(--panel-raised);
  color: var(--muted);
  font-size: 10px;
}
</style>
