<template>
  <SelectRoot
    v-model="modelValue"
    v-model:open="open"
    :disabled="disabled || undefined"
  >
    <SelectTrigger
      class="ui-select ui-selector-trigger"
      :aria-label="ariaLabel || undefined"
      :style="
        maxWidth === undefined ? undefined : { maxWidth: `${maxWidth}px` }
      "
    >
      <span
        class="ui-select-value ui-selector-value"
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
            class="ui-menu-item ui-select-option ui-selector-option"
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
.ui-select-value[data-empty] {
  color: var(--faint);
}

:global(.ui-select-list) {
  min-width: var(--reka-select-trigger-width);
  max-height: min(320px, calc(100vh - 120px));
  overflow-y: auto;
  overscroll-behavior: contain;
}
</style>
