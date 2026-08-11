<script setup lang="ts">
defineProps<{
  name:
    | "archive"
    | "chevron"
    | "cross"
    | "folder"
    | "grip"
    | "plus"
    | "stop"
    | "triangle"
    | "trash";
}>();
</script>

<template>
  <svg viewBox="0 0 20 20" fill="none" aria-hidden="true">
    <path v-if="name === 'archive'" d="M3 4.5h14v3H3zm1.5 3v9h11v-9M8 10.5h4" />
    <path v-else-if="name === 'chevron'" d="m7 5 5 5-5 5" />
    <path v-else-if="name === 'cross'" d="m5.5 5.5 9 9m0-9-9 9" />
    <path v-else-if="name === 'folder'" d="M2.5 5.5h5l1.5 2h8.5v8h-15z" />
    <path
      v-else-if="name === 'grip'"
      d="M7 5h.01M13 5h.01M7 10h.01M13 10h.01M7 15h.01M13 15h.01"
    />
    <path v-else-if="name === 'plus'" d="M10 4v12M4 10h12" />
    <path v-else-if="name === 'stop'" d="M5 5h10v10H5z" />
    <path v-else-if="name === 'triangle'" d="M5 4.5 15.5 10 5 15.5Z" />
    <path
      v-else-if="name === 'trash'"
      d="M4 6h12M8 3h4l1 3H7zm-2 3 1 11h6l1-11"
    />
  </svg>
</template>

<style scoped>
svg {
  display: block;
  width: 1em;
  height: 1em;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}
</style>
