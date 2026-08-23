<template>
  <TranscriptNotice
    type="error"
    :label="label"
    :text="text"
    inline
  />
</template>

<script setup lang="ts">
import TranscriptNotice from './TranscriptNotice.vue';

withDefaults(defineProps<{ text: string; label?: string }>(), {
  label: 'Reply Failed',
});
</script>
