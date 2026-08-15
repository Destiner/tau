<template>
  <UiDialog
    v-model:open="open"
    :title="
      step === 'connection'
        ? 'SSH connection'
        : 'Choose remote working directory'
    "
    :width="step === 'connection' ? 'sm' : 'md'"
    :busy="connecting"
  >
    <form
      v-if="step === 'connection'"
      @submit.prevent="submitConnection"
    >
      <UiContextMenu
        :items="() => textFieldItems(() => connectionInput?.input)"
      >
        <UiInput
          ref="connectionInput"
          v-model="connectionString"
          variant="mono"
          :error="Boolean(connectionError)"
          type="text"
          inputmode="text"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="ssh user@example -p 1234"
          :aria-label="connectionError || 'SSH connection string'"
          :aria-invalid="Boolean(connectionError)"
          :title="connectionError"
          :readonly="connecting || mode === 'retry'"
        />
      </UiContextMenu>
    </form>

    <div
      v-else
      class="remote-directory-dialog"
    >
      <UiContextMenu
        :items="() => textFieldItems(() => directoryFilterInput?.input)"
      >
        <UiInput
          ref="directoryFilterInput"
          v-model="directoryFilter"
          variant="mono"
          :error="Boolean(connectionError)"
          type="text"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="Filter directories"
          aria-label="Filter remote directories"
          aria-controls="remote-directory-list"
          :aria-activedescendant="`remote-directory-option-${selectedIndex}`"
          :aria-invalid="Boolean(connectionError)"
          :title="connectionError"
          :readonly="connecting"
          @keydown="handleDirectoryKeydown"
        />
      </UiContextMenu>
      <div
        id="remote-directory-list"
        class="remote-directory-list"
        role="listbox"
      >
        <button
          v-for="(option, index) in directoryOptions"
          :id="`remote-directory-option-${index}`"
          :key="option.path"
          class="remote-directory-option"
          :class="{ selected: index === selectedIndex }"
          type="button"
          role="option"
          tabindex="-1"
          :aria-selected="index === selectedIndex"
          :disabled="connecting"
          :title="option.path"
          @mousedown.prevent
          @mouseenter="() => highlight(index)"
          @click="() => chooseDirectory(option)"
        >
          {{ option.name }}
        </button>
      </div>
    </div>
  </UiDialog>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

import textFieldItems from '../lib/text-menu';

import UiContextMenu from './ui/UiContextMenu.vue';
import UiDialog from './ui/UiDialog.vue';
import UiInput from './ui/UiInput.vue';

interface RemoteDirectoryOption {
  name: string;
  path: string;
  kind: 'back' | 'select' | 'forward';
}

const open = defineModel<boolean>('open', { default: false });
const connectionString = defineModel<string>('connectionString', {
  default: '',
});
const directoryFilter = defineModel<string>('directoryFilter', { default: '' });
const selectedIndex = defineModel<number>('selectedIndex', { default: 0 });

const props = defineProps<{
  step: 'connection' | 'directory';
  mode: 'add' | 'retry';
  connectionError: string;
  connecting: boolean;
  directoryOptions: RemoteDirectoryOption[];
}>();

const emit = defineEmits<{
  'submit-connection': [];
  'choose-directory': [path: string, kind: 'back' | 'select' | 'forward'];
}>();

const connectionInput = ref<InstanceType<typeof UiInput>>();
const directoryFilterInput = ref<InstanceType<typeof UiInput>>();

watch(
  () => [open.value, props.step] as const,
  ([isOpen, step]) => {
    if (!isOpen) return;
    void nextTick(() => {
      if (step === 'connection') connectionInput.value?.input?.focus();
      else directoryFilterInput.value?.input?.focus();
    });
  },
);

watch(directoryFilter, () => {
  selectedIndex.value = 0;
});

function submitConnection(): void {
  emit('submit-connection');
}

function highlight(index: number): void {
  selectedIndex.value = index;
}

function chooseDirectory(option: RemoteDirectoryOption): void {
  emit('choose-directory', option.path, option.kind);
}

function handleDirectoryKeydown(event: KeyboardEvent): void {
  if (props.connecting) return;
  const lastIndex = props.directoryOptions.length - 1;
  if (lastIndex < 0) return;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    selectedIndex.value = Math.min(
      lastIndex,
      Math.max(0, selectedIndex.value + delta),
    );
    scrollSelected();
    return;
  }
  if (event.key === 'Enter' && !event.isComposing) {
    event.preventDefault();
    const option = props.directoryOptions[selectedIndex.value];
    if (option) chooseDirectory(option);
  }
}

function scrollSelected(): void {
  void nextTick(() => {
    document
      .getElementById(`remote-directory-option-${selectedIndex.value}`)
      ?.scrollIntoView({ block: 'nearest' });
  });
}
</script>

<style scoped>
.remote-directory-dialog {
  gap: 6px;
}

.remote-directory-list {
  max-height: min(360px, calc(100vh - 150px));
  overflow-y: auto;
  overscroll-behavior: contain;
}

.remote-directory-option {
  display: block;
  width: 100%;
  height: 30px;
  padding: 0 9px;
  overflow: hidden;
  border-radius: 5px;
  background: transparent;
  color: var(--text);
  font-size: 12px;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remote-directory-option:hover,
.remote-directory-option.selected {
  background: var(--selected);
  color: var(--text);
}
</style>
