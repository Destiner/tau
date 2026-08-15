<template>
  <div
    class="tool-call"
    :class="{ expanded }"
  >
    <button
      type="button"
      class="tool-header"
      :disabled="!expandable"
      :aria-expanded="expandable ? expanded : undefined"
      @click="toggle"
    >
      <span class="tool-copy">
        <span class="tool-heading">
          <span class="tool-name">{{ entry.toolName || 'tool' }}</span>
          <UiIcon
            v-if="expandable"
            class="tool-disclosure"
            name="chevron"
          />
          <span
            v-if="entry.toolRunning"
            class="tool-status"
            role="status"
            aria-label="Running"
          ></span>
          <UiIcon
            v-else-if="entry.toolErrored"
            class="tool-status-failed"
            name="cross"
            aria-label="Failed"
          />
        </span>
        <span
          v-if="entry.text"
          class="tool-argument"
          >{{ entry.text }}</span
        >
      </span>
    </button>

    <div
      v-if="expanded && expandable"
      class="tool-details"
    >
      <div
        v-if="entry.toolArguments"
        class="tool-detail"
      >
        <span class="tool-detail-label">Arguments</span>
        <pre class="tool-detail-body">{{ entry.toolArguments }}</pre>
      </div>
      <div
        v-if="entry.toolResult"
        class="tool-detail"
      >
        <span class="tool-detail-label">{{
          entry.toolErrored ? 'Error' : 'Result'
        }}</span>
        <pre class="tool-detail-body">{{ entry.toolResult }}</pre>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';

import UiIcon from './UiIcon.vue';

const props = defineProps<{
  entry: TranscriptEntry;
  expanded: boolean;
}>();

const emit = defineEmits<{ toggle: [] }>();

function toggle(): void {
  emit('toggle');
}

/** A call the reader can open: one that has something beyond its summary line. */
const expandable = computed(() =>
  Boolean(props.entry.toolArguments || props.entry.toolResult),
);
</script>
