<template>
  <div class="activity-row">
    <UiCollapsible
      v-model:open="open"
      :disabled="!expandable"
    >
      <template #trigger>
        <button
          type="button"
          class="activity-header"
        >
          <span class="activity-name">{{ name }}</span>
          <span class="activity-argument">{{ argument }}</span>
          <!--
            The disclosure keeps its place whether or not the row has one, so
            the mark after it lands in the same column on every row that
            carries one. A row with nothing to report ends at the argument.
          -->
          <span class="activity-end">
            <span class="activity-disclosure-slot">
              <UiIcon
                v-if="expandable"
                class="activity-disclosure"
                :name="expanded ? 'chevron-up' : 'chevron-down'"
              />
            </span>
            <span
              v-if="entry.toolRunning"
              class="activity-mark running"
              role="status"
              aria-label="Running"
            ></span>
            <UiIcon
              v-else-if="entry.toolErrored"
              class="activity-mark failed"
              name="cross"
              role="status"
              aria-label="Failed"
            />
          </span>
        </button>
      </template>
      <template #content>
        <div
          class="activity-details"
          :class="{ 'activity-tool-details': entry.kind === 'tool' }"
        >
          <!--
            Reasoning and instructions are prose the reader asked to see, so
            they are rendered rather than shown as the payload they arrived in.
          -->
          <template v-if="entry.kind === 'thinking'">
            <MarkdownText
              class="activity-prose activity-thinking-prose"
              :source="entry.text"
              :base-path="basePath"
              :copy-paths="copyPaths"
            />
          </template>
          <template v-else-if="entry.kind === 'skill'">
            <div
              v-if="entry.skillPrompt"
              class="activity-detail"
            >
              <span class="activity-detail-label">Prompt</span>
              <MarkdownText
                class="activity-prose"
                :source="entry.skillPrompt"
              />
            </div>
            <div class="activity-detail">
              <span class="activity-detail-label">Instructions</span>
              <MarkdownText
                class="activity-prose"
                :source="entry.text"
              />
            </div>
          </template>
          <template v-else>
            <div
              v-if="entry.toolArguments"
              class="activity-detail"
            >
              <span class="activity-detail-label">Arguments</span>
              <pre class="activity-detail-body">{{ entry.toolArguments }}</pre>
            </div>
            <div
              v-if="entry.toolResult"
              class="activity-detail"
            >
              <span class="activity-detail-label">{{
                entry.toolErrored ? 'Error' : 'Result'
              }}</span>
              <pre
                class="activity-detail-body"
                :class="{ failed: entry.toolErrored }"
                >{{ entry.toolResult }}</pre>
            </div>
          </template>
        </div>
      </template>
    </UiCollapsible>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';

import MarkdownText from './ui/MarkdownText.vue';
import UiCollapsible from './ui/UiCollapsible.vue';
import UiIcon from './ui/UiIcon.vue';

const props = defineProps<{
  entry: TranscriptEntry;
  expanded: boolean;
  basePath?: string;
  copyPaths?: boolean;
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

/** What ran, in the name it is known by. */
const name = computed(() => {
  if (props.entry.kind === 'thinking') return 'thinking';
  if (props.entry.kind === 'skill') return 'skill';
  return props.entry.toolName || 'tool';
});

/** What it ran on, as much of it as the line holds. */
const argument = computed(() => {
  if (props.entry.kind === 'thinking') return '';
  if (props.entry.kind === 'skill') return props.entry.skillName ?? '';
  return props.entry.text;
});

/** A row the reader can open: one that has something beyond its summary line. */
const expandable = computed(() => {
  const entry = props.entry;
  if (entry.kind === 'thinking' || entry.kind === 'skill') {
    return Boolean(entry.text);
  }
  return Boolean(entry.toolArguments || entry.toolResult);
});
</script>

<style scoped>
.activity-header {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  width: 100%;
  min-width: 0;
  padding: 3px 6px;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  text-align: left;
  gap: 8px;
}

.activity-header:focus-visible,
.activity-header:enabled:hover {
  outline: 0;
  background: var(--hover);
}

.activity-end {
  display: flex;
  align-items: center;
  gap: 6px;
}

.activity-mark {
  flex: none;
  width: 11px;
  height: 11px;
}

.activity-mark.running {
  position: relative;
}

.activity-mark.running::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--status-working);
}

.activity-mark.failed {
  color: var(--danger);
  font-size: 11px;
}

.activity-name {
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-tight);
}

.activity-header:enabled:hover .activity-name {
  color: var(--text);
}

.activity-argument {
  overflow: hidden;
  color: var(--faint);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-tight);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.activity-disclosure-slot {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 12px;
}

/* The chevron points at what the click does: down to open, up to close. */
.activity-disclosure {
  flex: none;
  opacity: 0;
  color: var(--faint);
  font-size: 11px;
}

.activity-header:hover .activity-disclosure,
.activity-header:focus-visible .activity-disclosure {
  opacity: 1;
}

/*
 * What was opened hangs under the row that opened it, on a rule rather than in
 * a panel: the detail belongs to that line and nothing else on the canvas is a
 * card.
 */
.activity-details {
  display: flex;
  flex-direction: column;
  margin: 4px 0 2px 14px;
  padding-left: 10px;
  border-left: 1px solid var(--border);
  gap: 8px;
}

.activity-tool-details {
  max-height: min(520px, 65vh);
  overflow-y: auto;
}

.activity-detail {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 3px;
}

.activity-detail-label {
  color: var(--faint);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-tight);
}

.activity-detail-body {
  margin: 0;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
  white-space: pre-wrap;
  overscroll-behavior: contain;
  overflow-wrap: anywhere;
  cursor: text;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: text;
  user-select: text;
}

.activity-detail-body.failed {
  color: var(--danger);
}

/* MarkdownText owns the rendered element, so the scoped rule must cross the
 * component boundary rather than styling the fallthrough class locally. */
.activity-row :deep(.markdown.activity-prose) {
  max-height: min(520px, 65vh);
  overflow: auto;
  color: var(--muted);
  font-size: var(--text-sm);
  overscroll-behavior: contain;
}

/* OpenAI reasoning summaries arrive wrapped in Markdown bold. Inside thinking,
 * that wrapper is structure rather than emphasis, and separate paragraphs are
 * one compact train of thought rather than prose blocks. */
.activity-row :deep(.markdown.activity-thinking-prose strong) {
  color: inherit;
  font-weight: inherit;
}

.activity-row :deep(.markdown.activity-thinking-prose p) {
  margin: 0;
}
</style>
