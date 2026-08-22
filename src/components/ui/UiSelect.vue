<template>
  <span
    class="ui-select"
    :style="maxWidth === undefined ? undefined : { maxWidth: `${maxWidth}px` }"
  >
    <select
      class="ui-select-field"
      :value="modelValue"
      :disabled="disabled || undefined"
      :aria-label="ariaLabel || undefined"
      @change="handleChange"
    >
      <option
        v-if="showPlaceholder"
        value=""
      >
        {{ placeholder }}
      </option>
      <option
        v-else-if="needsFallback"
        :value="modelValue"
      >
        {{ fallbackLabel }}
      </option>
      <option
        v-for="option in options"
        :key="option.value"
        :value="option.value"
      >
        {{ option.label }}
      </option>
    </select>
  </span>
</template>

<script setup lang="ts">
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

const showPlaceholder = computed(
  () => !modelValue.value && Boolean(props.placeholder),
);
const needsFallback = computed(
  () =>
    Boolean(modelValue.value) &&
    !props.options.some((option) => option.value === modelValue.value),
);

function handleChange(event: Event): void {
  modelValue.value = (event.target as HTMLSelectElement).value;
}
</script>

<style scoped>
.ui-select {
  display: inline-grid;
  position: relative;
  grid-template-areas: 'field';
  min-width: 0;
  color: var(--muted);
}

.ui-select::after {
  content: '';
  z-index: 1;
  grid-area: field;
  place-self: center end;
  width: 5px;
  height: 5px;
  margin-right: 8px;
  transform: translateY(-1px) rotate(45deg);
  border-right: 1px solid currentcolor;
  border-bottom: 1px solid currentcolor;
  pointer-events: none;
}

.ui-select:has(select:disabled)::after {
  opacity: 0.45;
}

.ui-select-field {
  grid-area: field;
  width: auto;
  min-width: 0;
  height: var(--control-sm);
  padding: 0 24px 0 5px;
  overflow: hidden;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  outline: 0;
  background: transparent;
  color: var(--muted);
  font-size: var(--text-xs);
  text-overflow: ellipsis;
  appearance: none;
  field-sizing: content;
}

.ui-select-field:focus-visible {
  border-color: var(--border);
  background: var(--hover);
}

.ui-select-field:hover:not(:disabled) {
  border-color: var(--border);
  background: var(--hover);
}
</style>
