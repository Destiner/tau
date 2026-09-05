<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <button
        class="model-selector-trigger ui-selector-trigger"
        type="button"
        role="combobox"
        :disabled="disabled || undefined"
        :aria-label="ariaLabel || undefined"
        :aria-expanded="open"
        aria-haspopup="listbox"
      >
        <span class="ui-selector-value">{{ valueLabel }}</span>
      </button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent
        class="model-selector-popover ui-surface"
        side="top"
        align="start"
        :side-offset="5"
        aria-label="Models"
        @open-auto-focus="handleOpenAutoFocus"
      >
        <header
          v-if="models.length > 0"
          class="model-selector-search"
        >
          <input
            ref="searchInput"
            v-model="query"
            type="search"
            autocomplete="off"
            spellcheck="false"
            placeholder="Search"
            aria-label="Search Models"
            :aria-controls="listId"
            :aria-activedescendant="
              cursorModel ? optionId(cursorModel) : undefined
            "
            @keydown="handleSearchKeydown"
          />
        </header>
        <div
          :id="listId"
          class="model-selector-list"
          role="listbox"
          aria-label="Models"
        >
          <template
            v-for="group in groups"
            :key="group.provider"
          >
            <div class="model-selector-group-heading">
              {{ group.label }}
            </div>
            <button
              v-for="model in group.models"
              :id="optionId(model)"
              :key="modelKey(model)"
              class="model-selector-option ui-selector-option"
              type="button"
              role="option"
              tabindex="-1"
              :data-current="isCurrent(model) || undefined"
              :data-cursor="isCursor(model) || undefined"
              :aria-selected="isCurrent(model)"
              @mousedown.prevent
              @click="() => selectModel(model)"
            >
              {{ model.name }}
            </button>
          </template>
          <p
            v-if="displayedModels.length === 0"
            class="model-selector-empty"
          >
            {{ query ? 'No matching models' : 'No models available' }}
          </p>
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<script setup lang="ts">
import {
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from 'reka-ui';
import { computed, nextTick, ref, useId, watch } from 'vue';

import { flattenModelGroups, groupModels } from '../lib/model-selector';
import type { ModelOption } from '../lib/pi/model-scope';

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

const selectorId = useId();
const listId = `${selectorId}-list`;
const query = ref('');
const cursorKey = ref('');
const searchInput = ref<HTMLInputElement>();
const groups = computed(() => groupModels(props.models, query.value));
const displayedModels = computed(() => flattenModelGroups(groups.value));
const cursorModel = computed(() =>
  displayedModels.value.find((model) => modelKey(model) === cursorKey.value),
);
const valueLabel = computed(() => {
  if (props.models.length === 0) return 'No Models';
  const model = props.models.find(
    (option) => modelKey(option) === modelValue.value,
  );
  return model?.name ?? props.fallbackLabel ?? modelValue.value;
});

watch(open, (isOpen) => {
  if (isOpen) return;
  query.value = '';
  cursorKey.value = '';
});

watch(query, (value) => {
  const first = value.trim() ? displayedModels.value[0] : undefined;
  cursorKey.value = first ? modelKey(first) : '';
});

watch(displayedModels, (models) => {
  if (!models.some((model) => modelKey(model) === cursorKey.value)) {
    cursorKey.value = '';
  }
});

function modelKey(model: ModelOption): string {
  return `${model.provider}/${model.id}`;
}

function optionId(model: ModelOption): string {
  return `${selectorId}-${modelKey(model).replaceAll(/[^a-zA-Z0-9_-]/g, '-')}`;
}

function isCurrent(model: ModelOption): boolean {
  return modelKey(model) === modelValue.value;
}

function isCursor(model: ModelOption): boolean {
  return modelKey(model) === cursorKey.value;
}

function selectModel(model: ModelOption): void {
  modelValue.value = modelKey(model);
  open.value = false;
}

function handleOpenAutoFocus(event: Event): void {
  if (props.models.length === 0) return;
  event.preventDefault();
  void nextTick(() => searchInput.value?.focus());
}

function handleSearchKeydown(event: KeyboardEvent): void {
  if (event.isComposing) return;
  if (
    event.key !== 'ArrowDown' &&
    event.key !== 'ArrowUp' &&
    event.key !== 'Enter'
  ) {
    return;
  }
  event.preventDefault();
  if (event.key === 'Enter') {
    const model = cursorModel.value ?? displayedModels.value[0];
    if (model) selectModel(model);
    return;
  }
  if (displayedModels.value.length === 0) return;
  const index = displayedModels.value.findIndex(
    (model) => modelKey(model) === cursorKey.value,
  );
  const delta = event.key === 'ArrowDown' ? 1 : -1;
  const nextIndex =
    index < 0
      ? 0
      : Math.min(displayedModels.value.length - 1, Math.max(0, index + delta));
  const model = displayedModels.value[nextIndex];
  if (!model) return;
  cursorKey.value = modelKey(model);
  void nextTick(() =>
    document
      .getElementById(optionId(model))
      ?.scrollIntoView({ block: 'nearest' }),
  );
}
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- current value and traversal are intentionally independent states. */
:global(.model-selector-popover) {
  display: flex;
  z-index: 20;
  flex-direction: column;
  width: min(300px, calc(100vw - 18px));
  max-height: min(320px, calc(100vh - 120px));
  padding: 0;
  overflow: hidden;
  outline: 0;
}

.model-selector-search {
  flex: none;
  border-bottom: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--sunk);
}

.model-selector-search input {
  width: 100%;
  height: var(--control-lg);
  padding: 0 8px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  font-size: var(--text-sm);
}

.model-selector-search input::placeholder {
  color: var(--faint);
}

.model-selector-search input::-webkit-search-cancel-button {
  display: none;
}

.model-selector-list {
  min-height: 0;
  padding: 0 4px 4px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.model-selector-group-heading {
  display: flex;
  position: sticky;
  top: 0;
  align-items: center;
  height: var(--control-sm);
  margin-right: 10px;
  padding: 0 8px;
  background: var(--panel-raised);
  color: var(--faint);
  font-size: var(--text-xs);
}

.model-selector-option {
  display: block;
  width: 100%;
  height: var(--control-lg);
  padding: 0 8px;
  overflow: hidden;
  background: transparent;
  color: var(--text);
  font-size: var(--text-sm);
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-selector-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 48px;
  margin: 0;
  padding: 0 12px;
  color: var(--muted);
  font-size: var(--text-xs);
  font-weight: 400;
  text-align: center;
}
</style>
