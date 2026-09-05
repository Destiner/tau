<template>
  <SelectRoot
    v-model="modelValue"
    v-model:open="open"
    :disabled="disabled || undefined"
  >
    <SelectTrigger
      class="ui-select"
      :aria-label="ariaLabel || undefined"
      :style="
        maxWidth === undefined ? undefined : { maxWidth: `${maxWidth}px` }
      "
    >
      <span
        class="ui-select-value"
        :data-empty="valueLabel === placeholder || undefined"
        >{{ valueLabel }}</span
      >
    </SelectTrigger>
    <SelectPortal>
      <SelectContent
        class="ui-menu ui-select-list"
        position="popper"
        :side-offset="5"
      >
        <SelectViewport>
          <SelectItem
            v-for="option in options"
            :key="option.value"
            class="ui-menu-item ui-select-option"
            :value="option.value"
          >
            <SelectItemText>{{ option.label }}</SelectItemText>
          </SelectItem>
        </SelectViewport>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>

<script setup lang="ts">
import {
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectViewport,
} from 'reka-ui';
import { computed } from 'vue';

interface UiSelectOption {
  value: string;
  label: string;
}

const modelValue = defineModel<string>({ default: '' });
const open = defineModel<boolean>('open', { default: false });

const props = withDefaults(
  defineProps<{
    options: UiSelectOption[];
    /** Shown when there is no selection to display. */
    placeholder?: string;
    /** Shown when the value is not among the options (a stale selection). */
    fallbackLabel?: string;
    disabled?: boolean;
    ariaLabel?: string;
    maxWidth?: number;
  }>(),
  {
    placeholder: undefined,
    fallbackLabel: undefined,
    ariaLabel: undefined,
    maxWidth: 210,
  },
);

/*
 * The trigger prints the label itself rather than leaning on SelectValue, so a
 * value the model no longer offers still reads as something instead of blank.
 */
const valueLabel = computed(() => {
  if (!modelValue.value) return props.placeholder ?? '';
  const match = props.options.find(
    (option) => option.value === modelValue.value,
  );
  return match?.label ?? props.fallbackLabel ?? modelValue.value;
});
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, then focus. */

/*
 * The trigger is the one it has always been: transparent in the composer's
 * toolbar, a border and a wash under the pointer, a chevron drawn as a rotated
 * square. What changed is behind it — the list is the app's own menu surface
 * now, not the platform's popup, which ignored the palette and opened as a
 * sheet over the window.
 */
.ui-select {
  display: inline-flex;
  position: relative;
  align-items: center;
  min-width: 0;
  height: var(--control-sm);
  padding: 0 24px 0 5px;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  outline: 2px solid transparent;
  background: transparent;
  color: var(--muted);
  font-size: var(--text-xs);
}

.ui-select::after {
  content: '';
  position: absolute;
  top: 50%;
  right: 8px;
  width: 5px;
  height: 5px;
  transform: translateY(-4px) rotate(45deg);
  border-right: 1px solid currentcolor;
  border-bottom: 1px solid currentcolor;
  pointer-events: none;
}

.ui-select:disabled::after {
  opacity: 0.45;
}

.ui-select-value {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ui-select-value[data-empty] {
  color: var(--faint);
}

.ui-select:hover:not(:disabled) {
  border-color: var(--border);
  background: var(--hover);
}

.ui-select:focus-visible,
.ui-select[data-state='open'] {
  border-color: var(--accent);
  outline: 0;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 22%, transparent);
  color: var(--text);
}

/*
 * No column reserved for a checkmark: the option in force carries the quiet
 * selected wash, which is how a current row reads everywhere else, and a
 * three-item list is not indented for a mark only one row will ever show.
 */
:global(.ui-select-list) {
  min-width: var(--reka-select-trigger-width);
  max-height: min(320px, calc(100vh - 120px));
  overflow-y: auto;
  overscroll-behavior: contain;
}

:global(.ui-select-option[data-state='checked']) {
  background: var(--selected-inactive);
  color: var(--text);
  font-weight: 500;
}

:global(.ui-select-option[data-highlighted]) {
  background: var(--hover);
}

:global(.ui-select-option[data-state='checked'][data-highlighted]) {
  background: var(--selected);
}
</style>
