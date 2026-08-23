<template>
  <button
    ref="button"
    type="button"
    class="ui-icon-button"
    :data-size="size"
    :data-tone="tone"
    :data-variant="variant"
    :data-reveal="variant === 'reveal' || undefined"
    :aria-label="label"
    :disabled="disabled || undefined"
  >
    <slot />
  </button>
</template>

<script setup lang="ts">
import { ref } from 'vue';

withDefaults(
  defineProps<{
    /**
     * Aria-label; an icon button has no readable text of its own. A pointer
     * gets the same label from a `UiTooltip` around the button — the native
     * `title` is not used, so that one hover layer answers for the whole app.
     */
    label: string;
    disabled?: boolean;
    /** Box size: 28, 22, 20, or 16 px. */
    size?: 'lg' | 'md' | 'sm' | 'xs';
    /**
     * How the button behaves under the pointer: `fade` dims until hovered,
     * `fill` fills on hover, `reveal` is invisible until hovered (and until
     * its row is hovered, which the row's own stylesheet provides).
     */
    variant?: 'fade' | 'fill' | 'reveal';
    tone?: 'default' | 'danger';
  }>(),
  {
    size: 'md',
    variant: 'fade',
    tone: 'default',
  },
);

const button = ref<HTMLButtonElement>();

defineExpose({
  get button() {
    return button.value;
  },
});
</script>

<style scoped>
.ui-icon-button {
  display: grid;
  flex: none;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  outline: 2px solid transparent;
  place-items: center;
  background: transparent;
  color: var(--muted);
}

.ui-icon-button[data-size='lg'] {
  width: var(--control-lg);
  height: var(--control-lg);
  font-size: 16px;
}

.ui-icon-button[data-size='md'] {
  width: var(--control-md);
  height: var(--control-md);
  font-size: 12px;
}

.ui-icon-button[data-size='sm'] {
  width: var(--control-sm);
  height: var(--control-sm);
  font-size: 15px;
}

.ui-icon-button[data-size='xs'] {
  width: var(--control-xs);
  height: var(--control-xs);
  padding: 2px;
  font-size: 10px;
}

.ui-icon-button[data-variant='fade'] {
  opacity: 0.65;
}

.ui-icon-button[data-variant='reveal'] {
  opacity: 0;
}

/*
 * Focus is a ring, not only full opacity: a `reveal` button is invisible until
 * its row is hovered, so opacity alone left a keyboard with nothing to see.
 */
.ui-icon-button:focus-visible {
  outline: 0;
  opacity: 1;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 22%, transparent);
  color: var(--text);
}

.ui-icon-button[data-variant='fade']:hover:not(:disabled),
.ui-icon-button[data-variant='reveal']:hover:not(:disabled) {
  opacity: 1;
}

.ui-icon-button[data-variant='fill']:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}

.ui-icon-button[data-tone='danger']:focus-visible {
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--danger) 20%, transparent);
  color: var(--danger);
}

.ui-icon-button[data-tone='danger']:hover:not(:disabled) {
  color: var(--danger);
}

.ui-icon-button[data-variant='fill'][data-tone='danger'] {
  color: var(--danger);
}

.ui-icon-button[data-variant='fill'][data-tone='danger']:hover:not(:disabled) {
  background: color-mix(in srgb, var(--danger) 10%, transparent);
  color: var(--danger);
}
</style>
