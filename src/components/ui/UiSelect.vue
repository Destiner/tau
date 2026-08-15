<template>
  <SelectRoot
    v-model="modelValue"
    :disabled="disabled"
  >
    <SelectTrigger
      class="ui-select-trigger"
      :aria-label="ariaLabel || undefined"
      :style="
        maxWidth === undefined ? undefined : { maxWidth: `${maxWidth}px` }
      "
    >
      <SelectValue class="ui-select-value">{{ displayLabel }}</SelectValue>
      <SelectIcon as-child>
        <span
          class="ui-select-chevron"
          aria-hidden="true"
        ></span>
      </SelectIcon>
    </SelectTrigger>
    <SelectPortal>
      <SelectContent class="ui-select-content">
        <SelectViewport>
          <SelectItem
            v-for="option in options"
            :key="option.value"
            :value="option.value"
            class="ui-select-item"
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
  SelectIcon,
  SelectItem,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectValue,
  SelectViewport,
} from 'reka-ui';
import { computed } from 'vue';

interface UiSelectOption {
  value: string;
  label: string;
}

const modelValue = defineModel<string>({ default: '' });

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

const displayLabel = computed(() => {
  const selected = props.options.find(
    (option) => option.value === modelValue.value,
  );
  return selected?.label ?? props.fallbackLabel ?? props.placeholder ?? '';
});
</script>

<style scoped>
.ui-select-trigger {
  display: inline-grid;
  grid-template-areas: 'selector';
  width: fit-content;
  height: 22px;
  padding: 0 24px 0 5px;
  overflow: hidden;
  border: 1px solid transparent;
  border-radius: 4px;
  outline: 0;
  background: transparent;
  color: var(--muted);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ui-select-trigger:focus-visible {
  border-color: var(--border);
  background: var(--hover);
}

.ui-select-trigger:hover:not(:disabled) {
  border-color: var(--border);
  background: var(--hover);
}

.ui-select-value {
  grid-area: selector;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ui-select-chevron {
  grid-area: selector;
  place-self: center end;
  width: 5px;
  height: 5px;
  margin-right: 8px;
  transform: translateY(-1px) rotate(45deg);
  border-right: 1px solid currentcolor;
  border-bottom: 1px solid currentcolor;
  pointer-events: none;
}

.ui-select-content {
  max-height: var(--reka-select-content-available-height);
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel-raised);
  box-shadow: 0 10px 30px var(--shadow-soft);
}

.ui-select-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 6px 7px;
  border-radius: 5px;
  color: var(--text);
  font-size: 11px;
  text-align: left;
}

.ui-select-item[data-highlighted] {
  background: var(--selected);
}

.ui-select-item[data-disabled] {
  color: var(--faint);
}
</style>
