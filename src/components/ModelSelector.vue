<template>
  <UiSelect
    v-model="modelValue"
    v-model:open="open"
    :options="options"
    :placeholder="models.length === 0 ? 'No Models' : undefined"
    :fallback-label="fallbackLabel"
    :disabled="disabled"
    :aria-label="ariaLabel"
    options-label="Models"
    filterable
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import providerLabel from '../lib/model-selector';
import type { ModelOption } from '../lib/pi/model-scope';

import UiSelect from './ui/UiSelect.vue';

const modelValue = defineModel<string>({ default: '' });
const open = defineModel<boolean>('open', { default: false });

const props = withDefaults(
  defineProps<{
    models: ModelOption[];
    fallbackLabel?: string;
    disabled?: boolean;
    ariaLabel?: string;
  }>(),
  {
    fallbackLabel: undefined,
    ariaLabel: undefined,
  },
);

const options = computed(() =>
  props.models.map((model) => ({
    value: `${model.provider}/${model.id}`,
    label: model.name,
    group: providerLabel(model.provider),
    searchText: model.id,
  })),
);
</script>
