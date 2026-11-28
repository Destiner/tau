<template>
  <form
    ref="prompt"
    class="extension-prompt"
    role="dialog"
    aria-modal="false"
    :aria-label="title"
    :aria-busy="submitting || undefined"
    @keydown.capture="handleKeydown"
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
      @keydown.space.prevent
      @keyup.space.prevent
    >
      <UiListOption
        v-for="(option, index) in options"
        :id="`extension-prompt-option-${index}`"
        :key="`${index}:${option}`"
        class="extension-prompt-option ui-pick-row"
        :data-cursor="index === selectedIndex || undefined"
        :aria-selected="index === selectedIndex"
        :disabled="submitting || disabled"
        @focus="() => (selectedIndex = index)"
        @mouseenter="(event: MouseEvent) => hoverOption(event, index)"
        @click="() => chooseOption(option)"
      >
        {{ option }}
      </UiListOption>
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

    <footer
      class="extension-prompt-actions"
      @keydown.space="preventChoiceSpace"
      @keyup.space="preventChoiceSpace"
    >
      <template v-if="method === 'confirm'">
        <UiButton
          ref="primaryAction"
          variant="primary"
          type="button"
          :disabled="submitting || disabled"
          label="Confirm"
          @click="confirm"
        />
        <UiButton
          ref="negativeAction"
          variant="secondary"
          type="button"
          :disabled="submitting || disabled"
          label="No"
          @click="reject"
        />
      </template>
      <UiButton
        v-else-if="method === 'input' || method === 'editor'"
        variant="primary"
        type="submit"
        label="Submit"
        :disabled="submitting || disabled"
      />
      <UiButton
        variant="secondary"
        type="button"
        :disabled="submitting || disabled"
        label="Cancel"
        @click="cancel"
      />
    </footer>
  </form>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';

import useDisabledFocusRepair, {
  topModal,
} from '../composables/useDisabledFocusRepair';
import textFieldItems from '../lib/text-menu';

import MarkdownText from './ui/MarkdownText.vue';
import UiButton from './ui/UiButton.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiInput from './ui/UiInput.vue';
import UiListOption from './ui/UiListOption.vue';
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
const isChoice = computed(
  () => props.method === 'select' || props.method === 'confirm',
);
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
  if (isChoice.value) event.preventDefault();
}

function handleKeydown(event: KeyboardEvent): void {
  const composing =
    event.isComposing || (isChoice.value && event.keyCode === 229);
  if (isChoice.value && event.key === 'Enter' && (composing || event.repeat)) {
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

  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    moveChoiceFocus(event);
  }
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

function moveChoiceFocus(event: KeyboardEvent): void {
  const target = event.target;
  if (!(target instanceof HTMLButtonElement)) return;

  if (optionList.value?.contains(target)) {
    event.preventDefault();
    const options = optionList.value.querySelectorAll('button');
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    const next =
      (selectedIndex.value + delta + options.length) % options.length;
    options[next]?.focus({ preventScroll: true });
  } else if (props.method === 'confirm') {
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
