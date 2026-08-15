<template>
  <button
    ref="button"
    type="button"
    class="ui-button"
    :class="[variant, size]"
  >
    <slot />
  </button>
</template>

<script setup lang="ts">
import { ref } from 'vue';

withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary';
    size?: 'sm' | 'md';
  }>(),
  {
    variant: 'secondary',
    size: 'sm',
  },
);

const button = ref<HTMLButtonElement>();

/** The underlying element, for the imperative focus callers need. */
defineExpose({
  get button() {
    return button.value;
  },
});
</script>

<style scoped>
.ui-button {
  min-width: 54px;
  height: 26px;
  padding: 0 7px;
  border: 1px solid var(--border);
  border-radius: 5px;
  font-size: 11px;
}

.ui-button.secondary {
  background: transparent;
  color: var(--muted);
}

.ui-button.primary {
  border-color: var(--text);
  background: var(--text);
  color: var(--canvas);
}

.ui-button.md {
  height: 30px;
  padding: 0 10px;
  font-size: 12px;
}

.ui-button:hover,
.ui-button:focus-visible {
  outline: 0;
  filter: brightness(0.96);
}
</style>
