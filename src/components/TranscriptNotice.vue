<template>
  <div
    class="transcript-notice"
    :class="type"
  >
    <span class="notice-label">{{ label }}</span>
    <MarkdownText
      class="notice-message"
      :inline="inline"
      :source="text"
      :base-path="basePath"
      :copy-paths="copyPaths"
      :remote-project-path="remoteProjectPath"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { TranscriptNoticeType } from '../lib/pi/transcript';

import MarkdownText from './ui/MarkdownText.vue';

const props = withDefaults(
  defineProps<{
    type: TranscriptNoticeType;
    text: string;
    label?: string;
    basePath?: string;
    copyPaths?: boolean;
    remoteProjectPath?: string;
    inline?: boolean;
  }>(),
  {
    label: '',
    basePath: undefined,
    copyPaths: false,
    remoteProjectPath: undefined,
    inline: false,
  },
);

const label = computed(
  () =>
    props.label ||
    `${props.type.slice(0, 1).toLocaleUpperCase()}${props.type.slice(1)}`,
);
</script>

<style scoped>
.transcript-notice {
  --notice-tone: var(--muted);

  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 7px 10px;
  border: 1px solid color-mix(in srgb, var(--notice-tone) 32%, var(--border));
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--notice-tone) 7%, var(--panel-raised));
  gap: 2px;
}

.transcript-notice.warning {
  --notice-tone: var(--accent);
}

.transcript-notice.error {
  --notice-tone: var(--danger);
}

/*
 * The tone already says what kind of notice this is, so there is no icon: the
 * glyph was a second, smaller voice saying the same word as the label.
 */
.notice-label {
  overflow: hidden;
  color: var(--notice-tone);
  font-size: var(--text-notice-label);
  font-weight: 500;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.transcript-notice :deep(.notice-message) {
  min-width: 0;
  font-size: var(--text-md);
  line-height: 1.5;
}
</style>
