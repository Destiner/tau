<template>
  <div
    class="transcript-notice"
    :class="type"
  >
    <div class="notice-heading">
      <UiIcon
        class="notice-mark"
        :name="icon"
      />
      <span class="notice-label">{{ label }}</span>
    </div>
    <MarkdownText
      class="notice-message"
      :inline="inline"
      :source="text"
      :base-path="basePath"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { TranscriptNoticeType } from '../lib/pi/transcript';

import MarkdownText from './ui/MarkdownText.vue';
import UiIcon from './ui/UiIcon.vue';

const props = withDefaults(
  defineProps<{
    type: TranscriptNoticeType;
    text: string;
    label?: string;
    basePath?: string;
    inline?: boolean;
  }>(),
  {
    label: '',
    basePath: undefined,
    inline: false,
  },
);

const label = computed(
  () =>
    props.label ||
    `${props.type.slice(0, 1).toLocaleUpperCase()}${props.type.slice(1)}`,
);
const icon = computed<'cross' | 'info' | 'warning'>(() => {
  if (props.type === 'error') return 'cross';
  return props.type;
});
</script>

<style scoped>
.transcript-notice {
  --notice-tone: var(--muted);

  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid color-mix(in srgb, var(--notice-tone) 40%, var(--border));
  border-radius: 7px;
  background: color-mix(in srgb, var(--notice-tone) 6%, var(--panel-raised));
  gap: 3px;
}

.transcript-notice.warning {
  --notice-tone: var(--accent);
}

.transcript-notice.error {
  --notice-tone: var(--danger);
}

.notice-heading {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 6px;
}

.notice-mark {
  flex: none;
  color: var(--notice-tone);
  font-size: 10px;
}

.notice-label {
  overflow: hidden;
  color: var(--notice-tone);
  font-size: 9px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transcript-notice .notice-message {
  min-width: 0;
  font-size: 12px;
  line-height: 1.5;
}
</style>
