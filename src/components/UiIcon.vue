<script setup lang="ts">
defineProps<{
  name: "chevron" | "folder" | "plus" | "send" | "stop" | "trash";
}>();
</script>

<template>
  <svg viewBox="0 0 20 20" fill="none" aria-hidden="true">
    <path v-if="name === 'chevron'" d="m7 5 5 5-5 5" />
    <path v-else-if="name === 'folder'" d="M2.5 5.5h5l1.5 2h8.5v8h-15z" />
    <path v-else-if="name === 'plus'" d="M10 4v12M4 10h12" />
    <path v-else-if="name === 'send'" d="m3 3 14 7-14 7 2-7zm2 7h7" />
    <path v-else-if="name === 'stop'" d="M5 5h10v10H5z" />
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
