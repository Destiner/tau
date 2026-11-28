<template>
  <UiSelect
    v-model="modelValue"
    v-model:open="open"
    :options="options"
    :placeholder="models.length === 0 ? 'No Models' : undefined"
    :fallback-label="fallbackLabel"
    :disabled="disabled"
    :label="label"
    :tooltip="tooltip"
    options-label="Models"
    filterable
    @select="forwardSelect"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import providerLabel from '../lib/model-selector';
import type { ModelOption } from '../lib/pi/model-scope';

import UiSelect from './ui/UiSelect.vue';
import type { ActionTooltip } from './ui/action-tooltip';

const modelValue = defineModel<string>({ default: '' });
const open = defineModel<boolean>('open', { default: false });

const props = withDefaults(
  defineProps<{
    models: ModelOption[];
    fallbackLabel?: string;
    disabled?: boolean;
    label: string;
    tooltip?: ActionTooltip;
  }>(),
  {
    tooltip: undefined,
    fallbackLabel: undefined,
  },
);
const emit = defineEmits<{ select: [value: string] }>();

function forwardSelect(value: string): void {
  emit('select', value);
}

const options = computed(() =>
  props.models.map((model) => ({
    value: `${model.provider}/${model.id}`,
    label: model.name,
    group: providerLabel(model.provider),
    searchText: model.id,
  })),
);
</script>
