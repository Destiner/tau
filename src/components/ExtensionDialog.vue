<template>
  <form
    ref="prompt"
    class="extension-prompt"
    role="dialog"
    aria-modal="false"
    :aria-label="title"
    :aria-busy="submitting || undefined"
    @keydown.capture="handleKeydown"
    @keyup.capture="preventChoiceSpace"
    @submit.prevent="handleSubmit"
  >
    <MarkdownText
      class="extension-prompt-title"
      :source="title"
      :base-path="workingDirectory"
      :copy-paths="copyPaths"
      :remote-project-path="remoteProjectPath"
    />

    <MarkdownText
      v-if="message"
      class="extension-prompt-message"
      :source="message"
      :base-path="workingDirectory"
      :copy-paths="copyPaths"
      :remote-project-path="remoteProjectPath"
    />

    <div
      v-if="method === 'select'"
      ref="optionList"
      class="extension-prompt-options"
      role="listbox"
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
        :disabled="submitting || disabled"
        @focus="() => (selectedIndex = index)"
        @mouseenter="(event) => hoverOption(event, index)"
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
        :disabled="submitting || disabled"
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
        :disabled="submitting || disabled"
      />
    </UiContextMenu>

    <p
      v-if="error"
      class="extension-prompt-error"
      role="status"
    >
      {{ error }}
    </p>

    <footer class="extension-prompt-actions">
      <template v-if="method === 'confirm'">
        <UiButton
          ref="primaryAction"
          variant="primary"
          type="button"
          :disabled="submitting || disabled"
          @click="confirm"
        >
          Confirm
        </UiButton>
        <UiButton
          ref="negativeAction"
          variant="secondary"
          type="button"
          :disabled="submitting || disabled"
          @click="reject"
        >
          No
        </UiButton>
      </template>
      <UiButton
        v-else-if="method === 'input' || method === 'editor'"
        variant="primary"
        type="submit"
        :disabled="submitting || disabled"
      >
        Submit
      </UiButton>
      <UiButton
        ref="cancelAction"
        variant="secondary"
        type="button"
        :disabled="submitting || disabled"
        @click="cancel"
      >
        Cancel
      </UiButton>
    </footer>
  </form>
</template>

<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';

import useDisabledFocusRepair, {
  topModal,
} from '../composables/useDisabledFocusRepair';
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
  submitting: boolean;
  disabled?: boolean;
  error: string;

  workingDirectory?: string;
  copyPaths?: boolean;
  remoteProjectPath?: string;
}>();

const emit = defineEmits<{
  submit: [value: string | boolean];
  cancel: [];
}>();

const dialogInput = ref<
  InstanceType<typeof UiInput> | InstanceType<typeof UiTextarea>
>();
const prompt = ref<HTMLFormElement>();
const optionList = ref<HTMLDivElement>();
const primaryAction = ref<InstanceType<typeof UiButton>>();
const negativeAction = ref<InstanceType<typeof UiButton>>();
const cancelAction = ref<InstanceType<typeof UiButton>>();
const selectedIndex = ref(0);
let escapeClaimed = false;

/**
 * Focus never scrolls: the prompt sits at the end of the transcript, which
 * brings itself to it, and a browser scrolling to a control instead would land
 * on the control rather than on the question above it.
 */
let initialFocusHandled = false;
function focusInitially(): void {
  if (initialFocusHandled) return;
  if (topModal()) {
    initialFocusHandled = true;
    return;
  }
  if (props.disabled || props.submitting) return;
  initialFocusHandled = true;
  focusDialog();
}
onMounted(() => {
  void nextTick(focusInitially);
});
watch(
  () => Boolean(props.disabled) || props.submitting,
  () => void nextTick(focusInitially),
);

useDisabledFocusRepair(
  () => prompt.value,
  () => props.submitting || Boolean(props.disabled),
  focusDialog,
);

watch(
  () => props.submitting,
  (submitting, wasSubmitting) => {
    if (!submitting && wasSubmitting) {
      escapeClaimed = false;
    }
  },
);

function focusDialog(): void {
  if (props.submitting || props.disabled) return;
  if (props.method === 'select') {
    const options = optionList.value?.querySelectorAll('button');
    (
      options?.[selectedIndex.value] ??
      options?.[0] ??
      prompt.value?.querySelector<HTMLButtonElement>('button:not(:disabled)')
    )?.focus({ preventScroll: true });
  } else if (props.method === 'confirm') {
    primaryAction.value?.button?.focus({ preventScroll: true });
  } else {
    dialogInput.value?.input?.focus({ preventScroll: true });
  }
}

function hoverOption(event: MouseEvent, index: number): void {
  selectedIndex.value = index;
  if (
    optionList.value?.contains(document.activeElement) &&
    event.currentTarget instanceof HTMLButtonElement
  ) {
    event.currentTarget.focus({ preventScroll: true });
  }
}

function chooseOption(value: string): void {
  if (!props.submitting && !props.disabled) emit('submit', value);
}

function confirm(): void {
  if (!props.submitting && !props.disabled) emit('submit', true);
}

function reject(): void {
  if (!props.submitting && !props.disabled) emit('submit', false);
}

function cancel(): void {
  if (!props.submitting && !props.disabled) emit('cancel');
}

function preventChoiceSpace(event: KeyboardEvent): void {
  const target = event.target;
  if (event.key !== ' ' || !(target instanceof HTMLButtonElement)) return;
  if (props.method !== 'select' && props.method !== 'confirm') return;
  if (
    optionList.value?.contains(target) ||
    target === primaryAction.value?.button ||
    target === negativeAction.value?.button ||
    target === cancelAction.value?.button
  ) {
    event.preventDefault();
  }
}

function handleKeydown(event: KeyboardEvent): void {
  preventChoiceSpace(event);
  const choice = props.method === 'select' || props.method === 'confirm';
  const composing = event.isComposing || (choice && event.keyCode === 229);
  if (choice && event.key === 'Enter' && (composing || event.repeat)) {
    event.preventDefault();
    event.stopPropagation();
    return;
  }
  if (composing) return;

  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopImmediatePropagation();
    if (escapeClaimed || props.submitting || props.disabled) return;

    escapeClaimed = true;
    cancel();
    return;
  }

  if (props.submitting || props.disabled) return;

  if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    event.stopPropagation();
    if (props.method === 'confirm') {
      if (event.shiftKey) reject();
      else confirm();
    } else if (props.method === 'input' || props.method === 'editor') {
      handleSubmit();
    }
    return;
  }

  if (choice) handleChoiceKeydown(event);
}

function handleSubmit(): void {
  if (
    props.submitting ||
    props.disabled ||
    (props.method !== 'input' && props.method !== 'editor')
  ) {
    return;
  }
  emit('submit', draft.value);
}

function handleChoiceKeydown(event: KeyboardEvent): void {
  const target = event.target;
  if (!(target instanceof HTMLButtonElement)) return;

  const moving = event.key === 'ArrowDown' || event.key === 'ArrowUp';
  if (props.method === 'select' && optionList.value?.contains(target)) {
    if (event.key === 'Enter') {
      event.preventDefault();
      const value = props.options?.[selectedIndex.value];
      if (value !== undefined) chooseOption(value);
    } else if (moving) {
      event.preventDefault();
      const options = optionList.value.querySelectorAll('button');
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      const next =
        (selectedIndex.value + delta + options.length) % options.length;
      selectedIndex.value = next;
      options[next]?.focus({ preventScroll: true });
    }
  } else if (props.method === 'confirm' && moving) {
    const confirmButton = primaryAction.value?.button;
    const noButton = negativeAction.value?.button;
    if (target !== confirmButton && target !== noButton) return;
    event.preventDefault();
    (target === confirmButton ? noButton : confirmButton)?.focus({
      preventScroll: true,
    });
  }
}
</script>

<style scoped>
.extension-prompt {
  display: flex;
  flex-direction: column;
  padding: 9px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--panel-raised);
  gap: 8px;
}

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

.extension-prompt-option:not([data-cursor]):is(:hover, :focus-visible) {
  background: transparent;
}

.extension-prompt-option:focus-visible:not([data-cursor])::before {
  content: none;
}

.extension-prompt-empty {
  padding: 5px 8px;
  color: var(--muted);
  font-size: var(--text-sm);
}

.extension-prompt-error {
  margin: 0;
  color: var(--danger);
  font-size: var(--text-xs);
}

.extension-prompt-actions {
  display: flex;
  justify-content: flex-start;
  gap: 4px;
}
</style>
