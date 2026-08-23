<template>
  <form
    class="extension-prompt"
    role="dialog"
    aria-modal="false"
    :aria-label="title"
    @submit.prevent="handleSubmit"
  >
    <MarkdownText
      class="extension-prompt-title"
      inline
      :source="title"
      :base-path="workingDirectory"
    />

    <MarkdownText
      v-if="message"
      class="extension-prompt-message"
      :source="message"
      :base-path="workingDirectory"
    />

    <div
      v-if="method === 'select'"
      class="extension-prompt-options"
      role="listbox"
      @keydown="handleSelectKeydown"
    >
      <button
        v-for="(option, index) in options"
        :id="`extension-prompt-option-${index}`"
        :key="`${index}:${option}`"
        class="extension-prompt-option ui-pick-row"
        type="button"
        role="option"
        :data-cursor="index === selectedIndex || undefined"
        :aria-selected="index === selectedIndex"
        @mouseenter="() => highlight(index)"
        @click="() => chooseOption(option)"
      >
        {{ option }}
      </button>
      <div
        v-if="options?.length === 0"
        class="extension-prompt-empty"
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

    <footer class="extension-prompt-actions">
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

/**
 * Focus never scrolls: the prompt sits at the end of the transcript, which
 * brings itself to it, and a browser scrolling to a control instead would land
 * on the control rather than on the question above it.
 */
onMounted(() => {
  void nextTick(() => {
    if (props.method === 'select') {
      document
        .getElementById('extension-prompt-option-0')
        ?.focus({ preventScroll: true });
    } else if (props.method === 'confirm') {
      primaryAction.value?.button?.focus({ preventScroll: true });
    } else {
      dialogInput.value?.input?.focus({ preventScroll: true });
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
      .getElementById(`extension-prompt-option-${selectedIndex.value}`)
      ?.focus();
  }
}
</script>

<style scoped>
/*
 * The prompt is a row of the transcript, so it has no height of its own to
 * bound and no scrolling region: the transcript scrolls, and a question longer
 * than the pane is read the way a long message is.
 */
.extension-prompt {
  display: flex;
  flex-direction: column;
  padding: 9px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  gap: 8px;
}

/*
 * A prompt is a sentence a workflow is saying, not a document's heading, so
 * its title is set as the text around it and separated by colour alone.
 */
.extension-prompt-title {
  color: var(--text);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

.extension-prompt-message {
  color: var(--muted);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

.extension-prompt-options {
  min-height: 32px;
}

/*
 * One row for the question's answers, on the app's pick-row rules: hover says
 * the pointer is here, the cursor says the keyboard is. The option is focused
 * as the arrows move, so focus takes the cursor's mark rather than a third one.
 */
.extension-prompt-option {
  display: block;
  width: 100%;
  padding: 5px 8px;
  background: transparent;
  color: var(--text);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
  text-align: left;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.extension-prompt-option:focus-visible {
  outline: 0;
}

.extension-prompt-empty {
  padding: 5px 8px;
  color: var(--muted);
  font-size: var(--text-sm);
}

.extension-prompt-actions {
  display: flex;
  justify-content: flex-start;
  gap: 4px;
}
</style>
