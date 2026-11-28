<template>
  <CheckboxRoot
    ref="checkbox"
    v-model="model"
    class="ui-checkbox"
    :aria-label="label"
    :disabled="disabled"
  >
    <span class="ui-checkbox-box">
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
    </span>
    <span>{{ label }}</span>
    <UiTooltip
      v-if="tooltip !== false"
      v-bind="tooltip"
      :anchor="checkboxElement"
      :disabled="disabled"
      :text="tooltip?.text ?? label"
    />
  </CheckboxRoot>
</template>

<script setup lang="ts">
import { CheckboxIndicator, CheckboxRoot } from 'reka-ui';
import { computed, ref } from 'vue';

import UiTooltip from './UiTooltip.vue';
import type { ActionTooltip } from './action-tooltip';

const model = defineModel<boolean>({ default: false });

withDefaults(
  defineProps<{
    label: string;
    disabled?: boolean;
    tooltip?: ActionTooltip;
  }>(),
  { tooltip: undefined },
);

const checkbox = ref<{ $el: unknown }>();
const checkboxElement = computed(() => {
  const element = checkbox.value?.$el;
  return element instanceof HTMLButtonElement ? element : undefined;
});
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- state rules are ordered by which should win: hover, then focus. */

.ui-checkbox {
  display: inline-flex;
  align-items: center;
  padding: 0;
  border: 0;
  outline: 0;
  background: transparent;
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

.ui-checkbox[data-state='checked'] .ui-checkbox-box {
  border-color: var(--accent);
  background: var(--accent);
}

.ui-checkbox[data-state='checked']:hover .ui-checkbox-box {
  border-color: color-mix(in srgb, var(--accent) 82%, var(--text));
  background: color-mix(in srgb, var(--accent) 82%, var(--text));
}

.ui-checkbox:focus-visible .ui-checkbox-box {
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

.ui-checkbox[data-state='unchecked'] .ui-checkbox-mark {
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
