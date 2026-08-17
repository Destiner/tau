<template>
  <!--
    The provider writes prose, and the address it tells the reader to visit is
    the one thing in it they have to act on, so the sentence is rendered as a
    run of text with its links live rather than as a flat string.
  -->
  <TranscriptNotice
    type="error"
    :label="description.label"
    :text="description.message"
    inline
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { describePiError } from '../lib/pi/error';

import TranscriptNotice from './TranscriptNotice.vue';

const props = defineProps<{ text: string }>();

const description = computed(() => describePiError(props.text));
</script>
