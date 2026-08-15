<template>
  <div class="error-notice">
    <div class="error-heading">
      <UiIcon
        class="error-mark"
        name="cross"
        aria-label="Error"
      />
      <span class="error-label">{{ description.label }}</span>
    </div>
    <!--
      The provider writes prose, and the address it tells the reader to visit is
      the one thing in it they have to act on, so the sentence is rendered as a
      run of text with its links live rather than as a flat string.
    -->
    <MarkdownText
      class="error-message"
      inline
      :source="description.message"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { describePiError } from '../lib/pi/error';

import MarkdownText from './ui/MarkdownText.vue';
import UiIcon from './ui/UiIcon.vue';

const props = defineProps<{ text: string }>();

const description = computed(() => describePiError(props.text));
</script>

<style scoped>
/*
 * A failed turn stands where the reply it replaced would have been, so it
 * carries the assistant's inset rather than the deeper one activity rows take.
 * The wash is the palette's error colour over the raised surface, which keeps
 * the panel legible in both schemes without a second colour to maintain.
 */
.error-notice {
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid color-mix(in srgb, var(--danger) 40%, var(--border));
  border-radius: 7px;
  background: color-mix(in srgb, var(--danger) 6%, var(--panel-raised));
  gap: 3px;
}

.error-heading {
  display: flex;
  align-items: center;
  min-width: 0;
  gap: 6px;
}

.error-mark {
  flex: none;
  color: var(--danger);
  font-size: 10px;
}

.error-label {
  overflow: hidden;
  color: var(--danger);
  font-size: 9px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/*
 * The sentence is the part worth reading, so it wraps at full width rather than
 * being clipped to a line the way a tool summary is. The selector is compound
 * to outrank the shared markdown rules in MarkdownText.
 */
.error-notice .error-message {
  min-width: 0;
  font-size: 12px;
  line-height: 1.5;
}
</style>
