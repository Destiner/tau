<template>
  <label
    class="ui-checkbox"
    :data-disabled="disabled || undefined"
  >
    <CheckboxRoot
      v-model="model"
      class="ui-checkbox-box"
      :disabled="disabled || undefined"
    >
      <CheckboxIndicator
        class="ui-checkbox-mark"
        force-mount
      >
        <svg
          viewBox="0 0 12 12"
          aria-hidden="true"
        >
          <path d="M2.6 6.3 4.8 8.6 9.4 3.6" />
        </svg>
      </CheckboxIndicator>
    </CheckboxRoot>
    <span><slot /></span>
  </label>
</template>

<script setup lang="ts">
import { CheckboxIndicator, CheckboxRoot } from 'reka-ui';

const model = defineModel<boolean>({ default: false });

withDefaults(defineProps<{ disabled?: boolean }>(), { disabled: false });
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, then focus. */

/*
 * Drawn by the app rather than by the platform. The OS box ignores the palette,
 * takes the global accent-color as its fill, and its radius is the one thing on
 * screen that looks like a web form. The tick is a stroked path, not a font
 * glyph, so it stays crisp at 13px.
 */
.ui-checkbox {
  display: inline-flex;
  align-items: center;
  color: var(--muted);
  font-size: var(--text-sm);
  gap: 7px;
}

.ui-checkbox[data-disabled] {
  color: var(--faint);
}

.ui-checkbox-box {
  display: grid;
  flex: none;
  width: 13px;
  height: 13px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-xs);
  outline: 2px solid transparent;
  place-items: center;
  background: transparent;
}

.ui-checkbox:hover .ui-checkbox-box {
  border-color: color-mix(in srgb, var(--muted) 45%, var(--border));
  background: var(--hover);
}

.ui-checkbox-box[data-state='checked'] {
  border-color: var(--accent);
  background: var(--accent);
}

.ui-checkbox:hover .ui-checkbox-box[data-state='checked'] {
  border-color: color-mix(in srgb, var(--accent) 82%, var(--text));
  background: color-mix(in srgb, var(--accent) 82%, var(--text));
}

.ui-checkbox-box:focus-visible {
  border-color: var(--accent);
  outline: 0;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 22%, transparent);
}

.ui-checkbox[data-disabled] .ui-checkbox-box {
  opacity: 0.45;
}

.ui-checkbox-mark {
  display: grid;
  width: 100%;
  height: 100%;
  place-items: center;
  color: var(--canvas);
}

.ui-checkbox-box[data-state='unchecked'] .ui-checkbox-mark {
  opacity: 0;
}

.ui-checkbox-mark svg {
  display: block;
  width: 100%;
  height: 100%;
  stroke-width: 1.9;
  stroke-linecap: round;
  stroke: currentcolor;
  fill: none;
  stroke-linejoin: round;
}
</style>
