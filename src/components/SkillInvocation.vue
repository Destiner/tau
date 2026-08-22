<template>
  <div
    class="skill-invocation"
    :class="{ expanded }"
  >
    <UiCollapsible
      v-model:open="open"
      :disabled="!expandable"
    >
      <template #trigger>
        <button
          type="button"
          class="skill-header"
        >
          <span class="skill-copy">
            <span class="skill-heading">
              <span class="skill-label">skill</span>
              <UiIcon
                v-if="expandable"
                class="skill-disclosure"
                :name="expanded ? 'chevron-up' : 'chevron-down'"
              />
            </span>
            <span class="skill-title">{{ entry.skillName || 'skill' }}</span>
          </span>
        </button>
      </template>
      <template #content>
        <div class="skill-details">
          <div
            v-if="entry.skillPrompt"
            class="skill-detail"
          >
            <span class="skill-detail-label">Prompt</span>
            <MarkdownText
              class="skill-detail-body"
              :source="entry.skillPrompt"
            />
          </div>
          <div class="skill-detail">
            <span class="skill-detail-label">Instructions</span>
            <MarkdownText
              class="skill-detail-body"
              :source="entry.text"
            />
          </div>
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
}>();

const emit = defineEmits<{ toggle: [] }>();

/** Expansion lives above the virtualized row so it survives an unmount. */
const open = computed({
  get: () => props.expanded,
  set: () => emit('toggle'),
});

const expandable = computed(() => Boolean(props.entry.text));
</script>

<style scoped>
.skill-invocation {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--panel-raised);
}

.skill-header {
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

.skill-header:focus-visible,
.skill-header:enabled:hover {
  outline: 0;
  background: var(--hover);
}

.skill-invocation.expanded .skill-header {
  border-radius: 5px 5px 0 0;
}

.skill-copy {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  gap: 2px;
}

.skill-heading {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 7px;
}

.skill-label {
  flex: 1;
  overflow: hidden;
  color: var(--muted);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 9px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.skill-title {
  overflow: hidden;
  color: var(--text);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.skill-disclosure {
  flex: none;
  opacity: 0;
  color: var(--faint);
  font-size: 11px;
}

.skill-header:hover .skill-disclosure,
.skill-header:focus-visible .skill-disclosure {
  opacity: 1;
}

.skill-details {
  display: flex;
  flex-direction: column;
  max-height: min(520px, 65vh);
  padding: 10px 11px;
  overflow: auto;
  border-top: 1px solid var(--border);
  font-size: 12px;
  overscroll-behavior: contain;
  gap: 10px;
}

.skill-detail {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 4px;
}

.skill-detail-label {
  color: var(--faint);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 9px;
  line-height: 1.2;
}

.skill-detail-body {
  font-size: 12px;
}
</style>
