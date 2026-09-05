<template>
  <p
    v-if="status"
    class="status"
    role="status"
  >
    {{ status }}
  </p>
  <div
    ref="composer"
    class="composer"
    :class="{ disabled: !canDraft }"
    @keydown.escape.prevent.stop="dismissCommandMenu"
  >
    <CommandMenu
      v-if="commandMenuActive"
      ref="commandMenu"
      :commands="filteredCommands"
      :selected-index="commandSelectedIndex"
      :placement="commandMenuPlacement"
      :max-height="commandMenuMaxHeight"
      :offset="commandMenuOffset"
      @highlight="handleHighlight"
      @select="selectCommand"
    />
    <UiContextMenu :items="() => textFieldItems(() => composerInput)">
      <textarea
        ref="composerInput"
        v-model="draft"
        rows="4"
        maxlength="32768"
        placeholder="Message π"
        :disabled="!canDraft"
        :aria-expanded="commandMenuActive"
        :aria-controls="commandMenuActive ? 'command-menu' : undefined"
        :aria-activedescendant="
          commandMenuActive && selectedCommand
            ? `command-option-${commandSelectedIndex}`
            : undefined
        "
        aria-autocomplete="list"
        aria-label="Message Pi"
        @keydown="handleComposerKeydown"
      ></textarea>
    </UiContextMenu>
    <div class="composer-toolbar">
      <ModelSelector
        v-model:open="modelSelectorOpen"
        :model-value="`${currentModelProvider}/${currentModelId}`"
        :models="models"
        :fallback-label="currentModelLabel"
        :disabled="settingsDisabled"
        aria-label="Model"
        @update:model-value="handleModelChange"
      />
      <UiSelect
        v-model:open="effortSelectorOpen"
        :model-value="currentEffort"
        :options="effortOptions"
        :fallback-label="currentEffortLabel"
        :disabled="settingsDisabled || efforts.length === 0"
        aria-label="Thinking Effort"
        :max-width="110"
        @update:model-value="handleEffortChange"
      />
      <UiTooltip
        v-if="streaming"
        text="Stop Pi"
      >
        <UiIconButton
          class="send-button stop"
          size="sm"
          variant="fill"
          tone="danger"
          :disabled="compacting || stopping || !canDraft"
          label="Stop Pi"
          @click="stop"
        >
          <UiIcon name="stop" />
        </UiIconButton>
      </UiTooltip>
      <UiTooltip
        v-else
        text="Send Message"
      >
        <UiIconButton
          class="send-button"
          size="sm"
          variant="fill"
          :disabled="!canCompose || !draft.trim()"
          label="Send Message"
          @click="send"
        >
          <UiIcon name="triangle" />
        </UiIconButton>
      </UiTooltip>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import useTau from '../composables/useTau';
import {
  type CommandMenuPlacement,
  type CommandOption,
  commandMenuLayout,
  commandSelection,
  filterCommands,
  slashCommandQuery,
} from '../lib/commands';
import type { ThinkingLevel } from '../lib/pi/model-scope';
import textFieldItems from '../lib/text-menu';

import CommandMenu from './CommandMenu.vue';
import ModelSelector from './ModelSelector.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiSelect from './ui/UiSelect.vue';
import UiTooltip from './ui/UiTooltip.vue';

const props = defineProps<{
  /** Resolves the session header, against which the command menu measures. */
  headerElement: () => HTMLElement | undefined;
}>();

const emit = defineEmits<{ send: [] }>();

const {
  canCompose,
  canDraft,
  commands,
  compacting,
  currentEffort,
  currentEffortLabel,
  currentModelId,
  currentModelLabel,
  currentModelProvider,
  draft,
  effortLabels,
  efforts,
  models,
  selectEffort,
  selectModel,
  settingsDisabled,
  status,
  stop,
  streaming,
  stopping,
} = useTau();

const composer = ref<HTMLElement>();
const composerInput = ref<HTMLTextAreaElement>();
const commandMenu = ref<InstanceType<typeof CommandMenu>>();
const commandMenuDismissed = ref(false);
const commandSelectedIndex = ref(0);
const commandMenuPlacement = ref<CommandMenuPlacement>('above');
const commandMenuMaxHeight = ref<number>();
const commandMenuOffset = ref(0);
const modelSelectorOpen = ref(false);
const effortSelectorOpen = ref(false);

const commandQuery = computed(() => slashCommandQuery(draft.value));
const filteredCommands = computed(() =>
  commandQuery.value === null
    ? []
    : filterCommands(commands.value, commandQuery.value),
);
const commandMenuActive = computed(
  () =>
    canDraft.value &&
    !commandMenuDismissed.value &&
    filteredCommands.value.length > 0,
);
const selectedCommand = computed(
  () => filteredCommands.value[commandSelectedIndex.value],
);
const effortOptions = computed(() =>
  efforts.value.map((effort) => ({
    value: effort,
    label: effortLabels[effort],
  })),
);

onMounted(() => {
  window.addEventListener('resize', updateCommandMenuLayout);
});
onBeforeUnmount(() => {
  window.removeEventListener('resize', updateCommandMenuLayout);
});

watch([commandQuery, commands], ([query]) => {
  commandSelectedIndex.value = 0;
  // A dismissed menu stays closed until the composer leaves the command it was
  // opened for, so Escape is not undone by the next keystroke.
  if (query === null) commandMenuDismissed.value = false;
});

watch(modelSelectorOpen, (open) => {
  if (open) effortSelectorOpen.value = false;
});

watch(effortSelectorOpen, (open) => {
  if (open) modelSelectorOpen.value = false;
});

watch([commandMenuActive, filteredCommands, status], () => {
  if (!commandMenuActive.value) return;
  void nextTick(updateCommandMenuLayout);
});

function updateCommandMenuLayout(): void {
  const menu = commandMenu.value?.menu;
  const anchor = composerInput.value;
  const container = composer.value;
  if (!menu || !anchor || !container) return;

  const anchorStyle = getComputedStyle(anchor);
  const anchorRect = anchor.getBoundingClientRect();
  const layout = commandMenuLayout({
    contentHeight: menu.scrollHeight + menu.offsetHeight - menu.clientHeight,
    composerTop: container.getBoundingClientRect().top,
    textTop: anchorRect.top + (Number.parseFloat(anchorStyle.paddingTop) || 0),
    textLineHeight: Number.parseFloat(anchorStyle.lineHeight) || 0,
    topBoundary: props.headerElement()?.getBoundingClientRect().bottom ?? 0,
    bottomBoundary: window.innerHeight,
  });

  commandMenuPlacement.value = layout.placement;
  commandMenuMaxHeight.value = layout.maxHeight;
  commandMenuOffset.value = layout.offset;
}

function handleComposerKeydown(event: KeyboardEvent): void {
  if (commandMenuActive.value && !event.isComposing) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const lastIndex = filteredCommands.value.length - 1;
      if (lastIndex < 0) return;
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      commandSelectedIndex.value = Math.min(
        lastIndex,
        Math.max(0, commandSelectedIndex.value + delta),
      );
      return;
    }
    if (event.key === 'Tab' && !event.shiftKey) {
      event.preventDefault();
      if (selectedCommand.value) selectCommand(selectedCommand.value, true);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (selectedCommand.value) selectCommand(selectedCommand.value);
      return;
    }
  }

  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    send();
  }
}

function handleHighlight(index: number): void {
  commandSelectedIndex.value = index;
}

function selectCommand(command: CommandOption, completeOnly = false): void {
  const selection = commandSelection(command, completeOnly);
  draft.value = selection.draft;
  commandSelectedIndex.value = 0;
  if (selection.submit) send();
}

function send(): void {
  emit('send');
}

function dismissCommandMenu(): void {
  if (commandMenuActive.value) commandMenuDismissed.value = true;
}

function handleModelChange(value: string): void {
  void selectModel(value);
}

function handleEffortChange(value: string): void {
  void selectEffort(value as ThinkingLevel);
}

function focus(): void {
  composerInput.value?.focus();
}

defineExpose({
  focus,
  get input() {
    return composerInput.value;
  },
});
</script>

<style scoped>
.status {
  width: 100%;
  margin: 0 0 6px;
  color: var(--muted);
  font-size: 11px;
  cursor: text;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: text;
  user-select: text;
}

/* stylelint-disable-next-line no-descending-specificity */
.composer {
  position: relative;
  width: 100%;
  margin: 0;
  background: transparent;
}

.composer.disabled {
  opacity: 0.68;
}

/*
 * The field sizes itself to its own content, between the four lines it opens
 * at and the ten it stops growing at. Measuring it from script instead means
 * collapsing it to its rows height on every keystroke, which rounds its height
 * to whole pixels and hands the space it gives up to the transcript for the
 * length of the measurement, clamping a reader sitting at the end away from it.
 */
/* stylelint-disable-next-line no-descending-specificity */
.composer textarea {
  display: block;
  width: 100%;
  min-height: calc(4lh + 7px);
  max-height: calc(10lh + 7px);
  padding: 4px 4px 3px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  line-height: 1.45;
  resize: none;
  field-sizing: content;
}

.composer textarea::placeholder {
  color: var(--faint);
}

.composer-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0;
}

.send-button {
  margin-left: auto;
}

/* Phosphor's stop and triangle read larger than the old glyphs at the same
 * box size, so both are pulled in a touch inside the send/stop button. */
.send-button svg {
  font-size: 13px;
}
</style>
