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
    variant?: 'primary' | 'secondary' | 'ghost';
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
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, then focus. */

/*
 * A button is as wide as its label. The old 54px floor is what made "No" look
 * like a form submit, and hover is an explicit color rather than a brightness
 * filter, which inverted its own meaning between the light and dark schemes.
 */
.ui-button {
  display: inline-flex;
  align-items: center;
  height: var(--control-md);
  padding: 0 9px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  outline: 2px solid transparent;
  font-size: var(--text-sm);
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
}

.ui-button.md {
  height: var(--control-lg);
  padding: 0 11px;
}

.ui-button.secondary,
.ui-button.ghost {
  background: transparent;
  color: var(--muted);
}

.ui-button.ghost {
  border-color: transparent;
}

.ui-button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: var(--on-accent);
}

.ui-button.secondary:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--muted) 45%, var(--border));
  background: var(--hover);
  color: var(--text);
}

.ui-button.ghost:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}

.ui-button.primary:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 88%, var(--on-accent));
  background: color-mix(in srgb, var(--accent) 88%, var(--on-accent));
}

/*
 * Focus is visible. `outline: 0` with nothing in its place left a keyboard
 * user with no cursor at all in a dialog whose buttons are the only controls.
 */
.ui-button:focus-visible {
  border-color: var(--accent);
  outline: 0;
  box-shadow: inset 0 0 0 1px var(--accent);
}

.ui-button.primary:focus-visible {
  border-color: var(--on-accent);
  box-shadow: inset 0 0 0 1px var(--on-accent);
}
</style>
