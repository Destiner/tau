<script setup lang="ts">
withDefaults(
  defineProps<{
    label?: string;
  }>(),
  { label: "Pi is working" },
);
</script>

<template>
  <span class="pi-spinner" role="status" :aria-label="label">
    <span
      v-for="index in 8"
      :key="index"
      class="pi-spinner-dot"
      :style="{ animationDelay: `${(8 - index) * -80}ms` }"
    ></span>
  </span>
</template>

<style scoped>
.pi-spinner {
  display: grid;
  width: 8px;
  height: 8px;
  flex: none;
  grid-template-columns: repeat(3, 2px);
  grid-template-rows: repeat(3, 2px);
  gap: 1px;
  color: var(--muted);
}

.pi-spinner-dot {
  width: 2px;
  height: 2px;
  animation: dot-trail 640ms steps(1, end) infinite;
  border-radius: 50%;
  background: currentColor;
}

.pi-spinner-dot:nth-child(1) {
  grid-area: 1 / 1;
}

.pi-spinner-dot:nth-child(2) {
  grid-area: 1 / 2;
}

.pi-spinner-dot:nth-child(3) {
  grid-area: 1 / 3;
}

.pi-spinner-dot:nth-child(4) {
  grid-area: 2 / 3;
}

.pi-spinner-dot:nth-child(5) {
  grid-area: 3 / 3;
}

.pi-spinner-dot:nth-child(6) {
  grid-area: 3 / 2;
}

.pi-spinner-dot:nth-child(7) {
  grid-area: 3 / 1;
}

.pi-spinner-dot:nth-child(8) {
  grid-area: 2 / 1;
}

@keyframes dot-trail {
  0% {
    opacity: 1;
  }

  12.5% {
    opacity: 0.72;
  }

  25% {
    opacity: 0.5;
  }

  37.5% {
    opacity: 0.34;
  }

  50% {
    opacity: 0.24;
  }

  62.5% {
    opacity: 0.17;
  }

  75% {
    opacity: 0.12;
  }

  87.5% {
    opacity: 0.08;
  }
}

@media (prefers-reduced-motion: reduce) {
  .pi-spinner-dot {
    animation: none;
    opacity: 0.45;
  }
}
</style>
