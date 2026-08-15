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

<style scoped>
.tool-call {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--panel-raised);
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.tool-header {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  padding: 7px 9px;
  border-radius: 5px;
  background: transparent;
  color: inherit;
  text-align: left;
}

.tool-header:focus-visible,
.tool-header:enabled:hover {
  outline: 0;
  background: var(--hover);
}

/* The open block already sits under its header, which cannot round into it. */
.tool-call.expanded .tool-header {
  border-radius: 5px 5px 0 0;
}

.tool-copy {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  gap: 2px;
}

.tool-heading {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 7px;
}

.tool-argument {
  overflow: hidden;
  color: var(--text);
  font-size: 11px;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tool-name {
  flex: 1;
  overflow: hidden;
  color: var(--muted);
  font-size: 9px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tool-status-failed {
  flex: none;
  color: var(--danger);
  font-size: 11px;
}

.tool-status {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--status-working);
}

/* The chevron points at what the click does: down to open, up to close. */
.tool-disclosure {
  flex: none;
  transform: rotate(90deg);
  transition: transform 120ms ease;
  opacity: 0;
  color: var(--faint);
  font-size: 11px;
}

.tool-header:hover .tool-disclosure,
.tool-header:focus-visible .tool-disclosure {
  opacity: 1;
}

.tool-call.expanded .tool-disclosure {
  transform: rotate(-90deg);
}

.tool-details {
  display: flex;
  flex-direction: column;
  padding: 9px;
  border-top: 1px solid var(--border);
  gap: 9px;
}

.tool-detail {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 4px;
}

.tool-detail-label {
  color: var(--faint);
  font-size: 9px;
  line-height: 1.2;
}

.tool-detail-body {
  max-height: 260px;
  margin: 0;
  padding: 8px 9px;
  overflow: auto;
  border-radius: 5px;
  background: var(--sunk);
  color: var(--text);
  font-family: inherit;
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  overscroll-behavior: contain;
  overflow-wrap: anywhere;
  cursor: text;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: text;
  user-select: text;
}
</style>
