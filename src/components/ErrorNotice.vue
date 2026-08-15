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
