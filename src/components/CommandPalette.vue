<template>
  <DialogRoot
    :open="open"
    @update:open="handleOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="command-palette-scrim" />
      <!-- App's blocking-surface selector must not treat the palette as an underlying dialog. -->
      <DialogContent
        class="command-palette"
        :data-state="undefined"
        :aria-label="pageTitle"
        @open-auto-focus="handleOpenAutoFocus"
        @close-auto-focus="handleCloseAutoFocus"
      >
        <DialogTitle class="command-palette-title-hidden">{{
          pageTitle
        }}</DialogTitle>
        <header class="command-palette-header">
          <button
            v-if="nested"
            class="command-palette-back"
            type="button"
            aria-label="Back"
            title="Back (⌘[)"
            @click="back"
          >
            <svg
              aria-hidden="true"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="m14 6-6 6 6 6" />
            </svg>
          </button>
          <svg
            v-else
            class="command-palette-search-icon"
            aria-hidden="true"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
          >
            <circle
              cx="10.8"
              cy="10.8"
              r="6.5"
            />
            <path d="m16 16 5 5" />
          </svg>
          <input
            ref="search"
            v-model="query"
            :placeholder="placeholder"
            :aria-label="pageTitle"
            role="combobox"
            aria-autocomplete="list"
            :aria-expanded="true"
            :aria-controls="resultsId"
            :aria-activedescendant="activeDescendant"
            @keydown="onKeydown"
          />
        </header>
        <div
          :id="resultsId"
          class="command-palette-results"
          role="listbox"
          :aria-label="pageTitle"
        >
          <button
            v-for="row in filteredRows"
            :id="optionId(row.id)"
            :key="row.id"
            type="button"
            role="option"
            class="command-palette-row"
            :class="{ selected: row.id === selectedId }"
            :aria-selected="row.id === selectedId"
            @mouseenter="() => (selectedId = row.id)"
            @click="() => select(row.id)"
          >
            <span class="command-palette-copy">
              <span class="command-palette-title">{{ row.title }}</span>
              <span
                v-if="row.detail"
                class="command-palette-detail"
                >{{ row.detail }}</span
              >
            </span>
            <kbd
              v-if="row.shortcut"
              :title="`Shortcut: ${row.shortcut}`"
              ><span
                v-for="(key, index) in shortcutKeys(row.shortcut)"
                :key="index"
                >{{ key }}</span
              ></kbd
            >
          </button>
          <p
            v-if="filteredRows.length === 0"
            class="command-palette-empty"
          >
            {{ emptyLabel }}
          </p>
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<script setup lang="ts">
import {
  DialogContent,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from 'reka-ui';
import { computed, getCurrentInstance, nextTick, ref, watch } from 'vue';

const open = defineModel<boolean>('open', { default: false });
const query = defineModel<string>('query', { default: '' });
const selectedId = defineModel<string | null>('selectedId', { default: null });

const props = withDefaults(
  defineProps<{
    rows: CommandPaletteRow[];
    pageTitle?: string;
    placeholder?: string;
    nested?: boolean;
  }>(),
  {
    pageTitle: 'Command Palette',
    placeholder: 'Search commands…',
    nested: false,
  },
);

const emit = defineEmits<{
  select: [id: string];
  back: [];
  close: [];
}>();

const search = ref<HTMLInputElement>();
const instanceId = getCurrentInstance()?.uid ?? 'palette';
const resultsId = `command-palette-results-${instanceId}`;

function fuzzyMatch(text: string, token: string): boolean {
  let position = 0;
  const candidate = text.toLocaleLowerCase();
  for (const character of token) {
    position = candidate.indexOf(character, position);
    if (position === -1) return false;
    position += 1;
  }
  return true;
}

const filteredRows = computed(() => {
  const tokens = query.value
    .toLocaleLowerCase()
    .trim()
    .split(/\s+/)
    .filter(Boolean);

  return props.rows.filter((row) =>
    tokens.every(
      (token) =>
        fuzzyMatch(row.title, token) ||
        Boolean(row.searchText?.toLocaleLowerCase().includes(token)),
    ),
  );
});

const emptyLabel = computed(() => {
  if (query.value.trim()) return 'No matches';
  switch (props.pageTitle) {
    case 'Switch Session':
      return 'No sessions in this project';
    case 'Switch Project':
      return 'No imported projects';
    case 'Choose Model':
      return 'No models available';
    case 'Choose Thinking Effort':
      return 'No thinking efforts available';
    default:
      return 'No commands available';
  }
});

const activeDescendant = computed(() =>
  selectedId.value &&
  filteredRows.value.some((row) => row.id === selectedId.value)
    ? optionId(selectedId.value)
    : undefined,
);

function shortcutKeys(shortcut: string): string[] {
  if (shortcut.includes('+')) return shortcut.split('+');
  const modifiers = shortcut.match(/^[⌘⌃⌥⇧]+/u)?.[0] ?? '';
  const key = shortcut.slice(modifiers.length);
  return [...Array.from(modifiers), ...(key ? [key] : [])];
}

function optionId(id: string): string {
  return `command-palette-option-${instanceId}-${id}`;
}

function ensureSelection(): void {
  if (
    filteredRows.value.length > 0 &&
    !filteredRows.value.some((row) => row.id === selectedId.value)
  )
    selectedId.value = filteredRows.value[0]?.id ?? null;
}

watch(filteredRows, ensureSelection, { immediate: true });
watch(open, async (isOpen) => {
  if (!isOpen) return;
  ensureSelection();
  await nextTick();
  search.value?.focus();
});
watch(selectedId, async (id) => {
  if (!id) return;
  await nextTick();
  document.getElementById(optionId(id))?.scrollIntoView({ block: 'nearest' });
});

function select(id: string): void {
  selectedId.value = id;
  emit('select', id);
}

function close(): void {
  if (!open.value) return;
  open.value = false;
  emit('close');
}

function handleOpenChange(next: boolean): void {
  if (!next) close();
}

function handleOpenAutoFocus(event: Event): void {
  event.preventDefault();
  search.value?.focus();
}

function handleCloseAutoFocus(event: Event): void {
  event.preventDefault();
}

function back(): void {
  emit('back');
}

function onKeydown(event: KeyboardEvent): void {
  if (event.isComposing) return;

  if (event.key === 'Escape') {
    event.preventDefault();
    close();
    return;
  }

  if (
    props.nested &&
    (((event.metaKey || event.ctrlKey) && event.key === '[') ||
      (!query.value &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        (event.key === 'Backspace' || event.key === 'Delete')))
  ) {
    event.preventDefault();
    back();
    return;
  }

  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const rows = filteredRows.value;
    if (rows.length === 0) return;
    const current = rows.findIndex((row) => row.id === selectedId.value);
    const offset = event.key === 'ArrowDown' ? 1 : -1;
    selectedId.value =
      rows[(current + offset + rows.length) % rows.length]?.id ?? null;
    return;
  }

  if (event.key === 'Enter') {
    event.preventDefault();
    ensureSelection();
    if (
      selectedId.value &&
      filteredRows.value.some((row) => row.id === selectedId.value)
    )
      select(selectedId.value);
  }
}

export type CommandPaletteRow = {
  id: string;
  title: string;
  shortcut?: string;
  detail?: string;
  searchText?: string;
};
</script>

<style scoped>
:global(.command-palette-scrim) {
  position: fixed;
  z-index: 101;
  background: var(--scrim);
  inset: 0;
}

:global(.command-palette) {
  display: flex;
  position: fixed;
  z-index: 102;
  top: 25vh;
  left: 50%;
  flex-direction: column;
  width: min(490px, calc(100% - 28px));
  max-height: min(520px, calc(75vh - 32px));
  overflow: hidden;
  transform: translateX(-50%);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  box-shadow: 0 12px 36px var(--shadow-strong);
}

.command-palette-title-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.command-palette-header {
  display: flex;
  align-items: center;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  gap: 6px;
}

.command-palette-header input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
}

.command-palette-header input::placeholder {
  opacity: 1;
  color: var(--muted);
}

.command-palette-search-icon,
.command-palette-back svg {
  flex: none;
  width: 12px;
  height: 12px;
  color: var(--muted);
}

.command-palette-back {
  display: grid;
  width: 12px;
  height: 12px;
  padding: 0;
  border: 0;
  place-items: center;
  background: transparent;
  cursor: pointer;
}

.command-palette-results {
  padding: 6px;
  overflow: auto;
  overscroll-behavior: contain;
}

.command-palette-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-width: 0;
  min-height: 28px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: var(--text-md);
  text-align: left;
  cursor: pointer;
  gap: 10px;
}

.command-palette-row.selected {
  background: var(--selected);
}

.command-palette-copy {
  display: flex;
  flex: 1;
  align-items: baseline;
  min-width: 0;
  gap: 8px;
}

.command-palette-title,
.command-palette-detail {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.command-palette-title {
  flex: 0 1 auto;
}

.command-palette-detail {
  flex: 1 1 0;
}

.command-palette-detail,
.command-palette-row kbd {
  color: var(--muted);
  font-size: var(--text-sm);
}

.command-palette-row kbd {
  display: inline-flex;
  flex: none;
  gap: 2px;
  padding: 0;
  border: 0;
  background: transparent;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.command-palette-empty {
  padding: 24px;
  color: var(--muted);
  text-align: center;
}
</style>
