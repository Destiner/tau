<template>
  <div
    class="tool-call"
    :class="{ expanded }"
  >
    <UiCollapsible
      v-model:open="open"
      :disabled="!expandable"
    >
      <template #trigger>
        <button
          type="button"
          class="tool-header"
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
      </template>
      <template #content>
        <div class="tool-details">
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
      </template>
    </UiCollapsible>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';

import UiCollapsible from './ui/UiCollapsible.vue';
import UiIcon from './ui/UiIcon.vue';

const props = defineProps<{
  entry: TranscriptEntry;
  expanded: boolean;
}>();

const emit = defineEmits<{ toggle: [] }>();

/**
 * The open state lives in the parent, which survives the virtualizer unmounting
 * the row; the collapsible only reflects it.
 */
const open = computed({
  get: () => props.expanded,
  set: () => emit('toggle'),
});

/** A call the reader can open: one that has something beyond its summary line. */
const expandable = computed(() =>
  Boolean(props.entry.toolArguments || props.entry.toolResult),
);
</script>
