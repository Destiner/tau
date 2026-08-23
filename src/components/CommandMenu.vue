<template>
  <div
    id="command-menu"
    ref="menu"
    class="command-menu ui-surface"
    :class="placement"
    :style="menuStyle"
    role="listbox"
    aria-label="Commands"
  >
    <button
      v-for="(command, index) in commands"
      :id="`command-option-${index}`"
      :key="`${command.source}:${command.name}`"
      class="command-option ui-pick-row"
      type="button"
      role="option"
      tabindex="-1"
      :data-cursor="index === selectedIndex || undefined"
      :aria-selected="index === selectedIndex"
      @mousedown.prevent
      @mouseenter="() => emit('highlight', index)"
      @click="() => emit('select', command)"
    >
      <span class="command-copy">
        <span class="command-name">/{{ command.name }}</span>
        <span
          v-if="command.description"
          class="command-description"
        >
          {{ command.description }}
        </span>
      </span>
      <span class="command-source">{{ command.source }}</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

import type { CommandOption, CommandMenuPlacement } from '../lib/commands';

const props = defineProps<{
  commands: CommandOption[];
  selectedIndex: number;
  placement: CommandMenuPlacement;
  maxHeight?: number;
  offset?: number;
}>();

const emit = defineEmits<{
  highlight: [index: number];
  select: [command: CommandOption];
}>();

const menu = ref<HTMLElement>();

const menuStyle = computed(() => ({
  maxHeight: props.maxHeight === undefined ? undefined : `${props.maxHeight}px`,
  top: props.placement === 'below' ? `${props.offset ?? 0}px` : undefined,
}));

watch(
  () => props.selectedIndex,
  () => {
    void nextTick(() => {
      document
        .getElementById(`command-option-${props.selectedIndex}`)
        ?.scrollIntoView({ block: 'nearest' });
    });
  },
);

defineExpose({
  get menu() {
    return menu.value;
  },
});
</script>

<style scoped>
.command-menu {
  position: absolute;
  z-index: 8;
  left: 4px;
  width: min(540px, calc(100% - 8px));
  max-height: min(300px, calc(100vh - 90px));
  overflow-y: auto;
  overscroll-behavior: contain;
}

.command-menu.above {
  bottom: calc(100% + 5px);
}

.command-option {
  display: flex;
  align-items: baseline;
  width: 100%;
  min-width: 0;
  padding: 5px 8px 6px;
  background: transparent;
  color: var(--text);
  text-align: left;
  gap: 10px;
}

.command-copy {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  gap: 2px;
}

.command-name,
.command-description {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.command-name {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-sm);
}

.command-description {
  color: var(--muted);
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
}

/* The source sits on the description's line, not on the command's. */
.command-source {
  flex: none;
  align-self: flex-end;
  color: var(--faint);
  font-size: var(--text-xs);
}
</style>
