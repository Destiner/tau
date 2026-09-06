<template>
  <PopoverRoot
    v-if="filterable"
    v-model:open="open"
  >
    <PopoverTrigger as-child>
      <button
        class="ui-select ui-selector-trigger"
        type="button"
        role="combobox"
        :disabled="disabled || undefined"
        :aria-label="ariaLabel || undefined"
        :aria-expanded="open"
        :style="triggerStyle"
        aria-haspopup="listbox"
      >
        <span
          class="ui-select-value ui-selector-value"
          :data-empty="valueLabel === placeholder || undefined"
          >{{ valueLabel }}</span
        >
      </button>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent
        class="ui-menu ui-select-list ui-select-filterable-list"
        side="top"
        align="start"
        :side-offset="5"
        :aria-label="optionsLabel"
        @open-auto-focus="handleOpenAutoFocus"
      >
        <header
          v-if="options.length > 0"
          class="ui-select-search"
        >
          <input
            ref="searchInput"
            v-model="query"
            type="search"
            autocomplete="off"
            spellcheck="false"
            placeholder="Search"
            :aria-label="`Search ${optionsLabel}`"
            :aria-controls="listId"
            :aria-activedescendant="
              cursorOption ? optionId(cursorOption) : undefined
            "
            @keydown="handleSearchKeydown"
          />
        </header>
        <div
          :id="listId"
          class="ui-select-filtered-options"
          role="listbox"
          :aria-label="optionsLabel"
        >
          <template
            v-for="group in groups"
            :key="group.key"
          >
            <div
              v-if="group.label"
              class="ui-select-group-heading"
            >
              {{ group.label }}
            </div>
            <button
              v-for="option in group.options"
              :id="optionId(option)"
              :key="option.value"
              class="ui-menu-item ui-select-option ui-select-filtered-option ui-selector-option"
              type="button"
              role="option"
              tabindex="-1"
              :data-current="option.value === modelValue || undefined"
              :data-cursor="option.value === cursorValue || undefined"
              :aria-selected="option.value === modelValue"
              @mousedown.prevent
              @click="() => selectOption(option)"
            >
              {{ option.label }}
            </button>
          </template>
          <p
            v-if="filteredOptions.length === 0"
            class="ui-select-empty"
          >
            {{ emptyLabel }}
          </p>
        </div>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>

  <SelectRoot
    v-else
    v-model="modelValue"
    v-model:open="open"
    :disabled="disabled || undefined"
  >
    <SelectTrigger
      class="ui-select ui-selector-trigger"
      :aria-label="ariaLabel || undefined"
      :style="triggerStyle"
    >
      <span
        class="ui-select-value ui-selector-value"
        :data-empty="valueLabel === placeholder || undefined"
        >{{ valueLabel }}</span
      >
    </SelectTrigger>
    <SelectPortal>
      <!-- Adjacent toolbar controls must remain clickable while this is open. -->
      <SelectContent
        class="ui-menu ui-select-list"
        position="popper"
        :side-offset="5"
        :body-lock="false"
        :disable-outside-pointer-events="false"
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
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectViewport,
} from 'reka-ui';
import { computed, nextTick, ref, useId, watch } from 'vue';

interface UiSelectOption {
  value: string;
  label: string;
  group?: string;
  searchText?: string;
}

interface UiSelectGroup {
  key: string;
  label?: string;
  options: UiSelectOption[];
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
    optionsLabel?: string;
    maxWidth?: number;
    /** Adds a search field and optional grouped results. */
    filterable?: boolean;
  }>(),
  {
    placeholder: undefined,
    fallbackLabel: undefined,
    ariaLabel: undefined,
    optionsLabel: undefined,
    maxWidth: 210,
    filterable: false,
  },
);

const selectId = useId();
const listId = `${selectId}-list`;
const query = ref('');
const cursorValue = ref('');
const searchInput = ref<HTMLInputElement>();
const triggerStyle = computed(() => ({ maxWidth: `${props.maxWidth}px` }));
const optionsLabel = computed(
  () => props.optionsLabel ?? `${props.ariaLabel ?? 'Option'}s`,
);
const filteredOptions = computed(() => {
  const normalized = query.value.trim().toLocaleLowerCase();
  if (!normalized) return props.options;
  return props.options.filter((option) =>
    `${option.label} ${option.searchText ?? ''}`
      .toLocaleLowerCase()
      .includes(normalized),
  );
});
const groups = computed<UiSelectGroup[]>(() => {
  const grouped = new Map<string, UiSelectGroup>();
  for (const option of filteredOptions.value) {
    const key = option.group ?? '';
    const group = grouped.get(key) ?? {
      key,
      label: option.group,
      options: [],
    };
    group.options.push(option);
    grouped.set(key, group);
  }
  return [...grouped.values()];
});
const displayedOptions = computed(() =>
  groups.value.flatMap((group) => group.options),
);
const cursorOption = computed(() =>
  displayedOptions.value.find((option) => option.value === cursorValue.value),
);
const emptyLabel = computed(() => {
  const noun = optionsLabel.value.toLocaleLowerCase();
  return query.value ? `No matching ${noun}` : `No ${noun} available`;
});

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

watch(open, (isOpen) => {
  if (isOpen) return;
  query.value = '';
  cursorValue.value = '';
});

watch(query, (value) => {
  cursorValue.value = value.trim()
    ? (displayedOptions.value[0]?.value ?? '')
    : '';
});

watch(displayedOptions, (options) => {
  if (!options.some((option) => option.value === cursorValue.value)) {
    cursorValue.value = '';
  }
});

function optionId(option: UiSelectOption): string {
  return `${selectId}-${option.value.replaceAll(/[^a-zA-Z0-9_-]/g, '-')}`;
}

function selectOption(option: UiSelectOption): void {
  modelValue.value = option.value;
  open.value = false;
}

function handleOpenAutoFocus(event: Event): void {
  if (props.options.length === 0) return;
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
    const option = cursorOption.value ?? displayedOptions.value[0];
    if (option) selectOption(option);
    return;
  }
  if (displayedOptions.value.length === 0) return;
  const index = displayedOptions.value.findIndex(
    (option) => option.value === cursorValue.value,
  );
  const delta = event.key === 'ArrowDown' ? 1 : -1;
  const nextIndex =
    index < 0
      ? 0
      : Math.min(displayedOptions.value.length - 1, Math.max(0, index + delta));
  const option = displayedOptions.value[nextIndex];
  if (!option) return;
  cursorValue.value = option.value;
  void nextTick(() =>
    document
      .getElementById(optionId(option))
      ?.scrollIntoView({ block: 'nearest' }),
  );
}
</script>

<style scoped>
/* stylelint-disable no-descending-specificity -- current value and traversal are intentionally independent states. */
.ui-select-value[data-empty] {
  color: var(--faint);
}

:global(.ui-select-list) {
  min-width: var(--reka-select-trigger-width);
  max-height: min(320px, calc(100vh - 120px));
  overflow-y: auto;
  overscroll-behavior: contain;
}

:global(.ui-menu.ui-select-filterable-list) {
  display: flex;
  z-index: 20;
  flex-direction: column;
  width: min(300px, calc(100vw - 18px));
  padding: 0;
  overflow: hidden;
  border: 0;
  outline: 0;
}

:global(.ui-select-search) {
  flex: none;
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  background: var(--sunk);
}

:global(.ui-select-search input) {
  width: 100%;
  height: var(--control-lg);
  padding: 0 12px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  font-size: var(--text-sm);
}

:global(.ui-select-search input::placeholder) {
  color: var(--faint);
}

:global(.ui-select-search input::-webkit-search-cancel-button) {
  display: none;
}

:global(.ui-select-filtered-options) {
  min-height: 0;
  padding: 0 4px 4px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

:global(.ui-select-group-heading) {
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

:global(.ui-select-filtered-option) {
  height: var(--control-lg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

:global(.ui-select-empty) {
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
