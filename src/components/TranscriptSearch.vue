<template>
  <div
    class="transcript-search"
    role="search"
    aria-label="Find in Transcript"
  >
    <div class="search-controls">
      <div class="search-field">
        <input
          ref="input"
          :value="query"
          aria-label="Find in Transcript"
          placeholder="Search"
          spellcheck="false"
          @input="handleInput"
          @keydown="handleKeydown"
        />
        <span
          class="match-count"
          aria-live="polite"
          >{{ status }}</span
        >
      </div>
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
}>();
const emit = defineEmits<{
  query: [value: string];
  navigate: [direction: number];
  close: [];
}>();
const input = ref<HTMLInputElement>();
const status = computed(() =>
  props.busy || props.pending || !props.query.trim()
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

.search-field {
  display: flex;
  flex: 1;
  align-items: center;
  min-width: 0;
}

.search-controls input {
  flex: 1;
  width: 0;
  min-width: 35px;
  height: var(--control-md);
  padding: 0 6px;
  border: 0;
  border-radius: 0;
  background: transparent;
  font-size: var(--text-md);
}

.search-controls input:focus {
  outline: none;
}

.search-controls :deep(svg) {
  width: 14px;
  height: 14px;
}

.match-count {
  flex-shrink: 0;
  width: 72px;
  padding-right: 6px;
  color: var(--muted);
  font-size: var(--text-sm);
  text-align: right;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
</style>
