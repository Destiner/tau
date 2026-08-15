<template>
  <form
    class="extension-composer"
    role="dialog"
    aria-modal="false"
    :aria-label="title"
    @submit.prevent="handleSubmit"
  >
    <header class="extension-dialog-header">
      <span class="extension-dialog-context">
        {{ projectName }} · {{ sessionName }}
      </span>
      <MarkdownText
        class="extension-dialog-title"
        inline
        :source="title"
        :base-path="workingDirectory"
      />
    </header>

    <MarkdownText
      v-if="message"
      class="extension-dialog-message"
      :source="message"
      :base-path="workingDirectory"
    />

    <div
      v-if="method === 'select'"
      class="extension-dialog-options"
      role="listbox"
      @keydown="handleSelectKeydown"
    >
      <button
        v-for="(option, index) in options"
        :id="`extension-dialog-option-${index}`"
        :key="`${index}:${option}`"
        class="extension-dialog-option"
        :class="{ selected: index === selectedIndex }"
        type="button"
        role="option"
        :aria-selected="index === selectedIndex"
        @mouseenter="() => highlight(index)"
        @click="() => chooseOption(option)"
      >
        {{ option }}
      </button>
      <div
        v-if="options?.length === 0"
        class="extension-dialog-empty"
      >
        No options available
      </div>
    </div>

    <UiContextMenu
      v-else-if="method === 'input'"
      :items="() => textFieldItems(() => dialogInput?.input)"
    >
      <UiInput
        ref="dialogInput"
        v-model="draft"
        type="text"
        autocomplete="off"
        :placeholder="placeholder"
        :aria-label="title"
      />
    </UiContextMenu>

    <UiContextMenu
      v-else-if="method === 'editor'"
      :items="() => textFieldItems(() => dialogInput?.input)"
    >
      <UiTextarea
        ref="dialogInput"
        v-model="draft"
        rows="6"
        :aria-label="title"
        @keydown.meta.enter.prevent="handleSubmit"
        @keydown.ctrl.enter.prevent="handleSubmit"
      />
    </UiContextMenu>

    <footer class="extension-dialog-actions">
      <template v-if="method === 'confirm'">
        <UiButton
          ref="primaryAction"
          variant="primary"
          type="button"
          @click="confirm"
        >
          Confirm
        </UiButton>
        <UiButton
          variant="secondary"
          type="button"
          @click="reject"
        >
          No
        </UiButton>
      </template>
      <UiButton
        v-else-if="method === 'input' || method === 'editor'"
        variant="primary"
        type="submit"
      >
        Submit
      </UiButton>
      <UiButton
        variant="secondary"
        type="button"
        @click="cancel"
      >
        Cancel
      </UiButton>
    </footer>
  </form>
</template>

<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';

import textFieldItems from '../lib/text-menu';

import MarkdownText from './ui/MarkdownText.vue';
import UiButton from './ui/UiButton.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiInput from './ui/UiInput.vue';
import UiTextarea from './ui/UiTextarea.vue';

const draft = defineModel<string>('draft', { default: '' });

const props = defineProps<{
  method: 'select' | 'confirm' | 'input' | 'editor';
  title: string;
  message?: string;
  options?: string[];
  placeholder?: string;
  projectName: string;
  sessionName: string;
  /** Base for the file paths in the text; absent when the project is remote. */
  workingDirectory?: string;
}>();

const emit = defineEmits<{
  submit: [value: string | boolean];
  cancel: [];
}>();

const dialogInput = ref<
  InstanceType<typeof UiInput> | InstanceType<typeof UiTextarea>
>();
const primaryAction = ref<InstanceType<typeof UiButton>>();
const selectedIndex = ref(0);

onMounted(() => {
  void nextTick(() => {
    if (props.method === 'select') {
      document.getElementById('extension-dialog-option-0')?.focus();
    } else if (props.method === 'confirm') {
      primaryAction.value?.button?.focus();
    } else {
      dialogInput.value?.input?.focus();
    }
  });
});

function highlight(index: number): void {
  selectedIndex.value = index;
}

function chooseOption(value: string): void {
  emit('submit', value);
}

function confirm(): void {
  emit('submit', true);
}

function reject(): void {
  emit('submit', false);
}

function cancel(): void {
  emit('cancel');
}

function handleSubmit(): void {
  if (props.method !== 'input' && props.method !== 'editor') return;
  emit('submit', draft.value);
}

function handleSelectKeydown(event: KeyboardEvent): void {
  const options = props.options ?? [];
  if (props.method !== 'select' || options.length === 0) return;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    selectedIndex.value =
      (selectedIndex.value + delta + options.length) % options.length;
    document
      .getElementById(`extension-dialog-option-${selectedIndex.value}`)
      ?.focus();
  }
}
</script>

<style scoped>
.extension-composer {
  display: flex;
  flex-direction: column;
  width: min(720px, 100%);
  max-height: min(520px, 60vh);
  margin: 0 auto;
  padding: 3px;
  gap: 8px;
}

.extension-dialog-header {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 4px;
}

/*
 * A prompt is a sentence a workflow is saying, not a document's heading, so
 * its title is set as the text around it and separated by colour alone.
 */
.extension-dialog-title {
  color: var(--text);
  font-size: 12px;
  line-height: 1.45;
}

.extension-dialog-message {
  overflow-y: auto;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.45;
  overscroll-behavior: contain;
}

.extension-dialog-context {
  overflow: hidden;
  color: var(--muted);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.extension-dialog-options {
  min-height: 32px;
  max-height: min(240px, 35vh);
  overflow-y: auto;
  overscroll-behavior: contain;
}

.extension-dialog-option {
  display: block;
  width: 100%;
  padding: 5px 7px;
  border-radius: 5px;
  background: transparent;
  color: var(--text);
  font-size: 12px;
  line-height: 1.4;
  text-align: left;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.extension-dialog-option:hover,
.extension-dialog-option:focus-visible,
.extension-dialog-option.selected {
  outline: 0;
  background: var(--selected);
}

.extension-dialog-empty {
  padding: 5px 7px;
  color: var(--muted);
  font-size: 12px;
}

.extension-dialog-actions {
  display: flex;
  justify-content: flex-start;
  gap: 4px;
}
</style>
