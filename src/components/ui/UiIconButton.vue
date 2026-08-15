<template>
  <button
    type="button"
    class="ui-icon-button"
    :data-size="size"
    :data-tone="tone"
    :data-variant="variant"
    :data-reveal="variant === 'reveal' || undefined"
    :aria-label="label"
    :title="title || undefined"
    :disabled="disabled || undefined"
  >
    <slot />
  </button>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    /** Aria-label; an icon button has no readable text of its own. */
    label: string;
    title?: string;
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
    title: undefined,
    variant: 'fade',
    tone: 'default',
  },
);
</script>

<style scoped>
.ui-icon-button {
  display: grid;
  flex: none;
  padding: 0;
  border: 0;
  border-radius: 4px;
  place-items: center;
  background: transparent;
  color: var(--muted);
}

.ui-icon-button[data-size='lg'] {
  width: 28px;
  height: 28px;
  font-size: 16px;
}

.ui-icon-button[data-size='md'] {
  width: 22px;
  height: 22px;
  font-size: 12px;
}

.ui-icon-button[data-size='sm'] {
  width: 20px;
  height: 20px;
  font-size: 15px;
}

.ui-icon-button[data-size='xs'] {
  width: 16px;
  height: 16px;
  border-radius: 3px;
  font-size: 10px;
}

.ui-icon-button[data-variant='fade'] {
  opacity: 0.65;
}

.ui-icon-button[data-variant='reveal'] {
  opacity: 0;
}

.ui-icon-button:focus-visible {
  outline: 0;
  opacity: 1;
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
