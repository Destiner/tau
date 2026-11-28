<template>
  <UiDialog
    v-model:open="open"
    :title="
      step === 'connection'
        ? 'SSH Connection'
        : 'Choose Remote Working Directory'
    "
    width="md"
    :busy="connecting"
    :return-focus="returnFocus"
  >
    <form
      v-if="step === 'connection'"
      class="remote-connection-form"
      @keydown="handleConnectionKeydown"
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
          aria-label="SSH Connection String"
          :aria-describedby="
            connectionError ? 'remote-connection-error' : undefined
          "
          :aria-invalid="Boolean(connectionError)"
          :readonly="connecting || mode === 'retry'"
        />
      </UiContextMenu>
      <p
        v-if="connectionError"
        id="remote-connection-error"
        class="remote-dialog-error"
        role="alert"
      >
        {{ connectionError }}
      </p>
      <ul
        v-if="mode === 'add' && recentDestinations.length"
        ref="recentList"
        class="remote-recent-list"
        role="list"
        aria-label="Recent SSH destinations"
        :aria-busy="forgetPending || undefined"
      >
        <li
          v-for="destination in recentDestinations"
          :key="destinationLabel(destination)"
          class="remote-recent-row"
        >
          <button
            type="button"
            class="remote-recent-destination ui-pick-row"
            :aria-disabled="recentActionsDisabled || undefined"
            :title="destinationLabel(destination)"
            @click="() => selectDestination(destination)"
          >
            {{ destinationLabel(destination) }}
          </button>
          <UiIconButton
            class="remote-recent-forget"
            size="lg"
            :label="`Forget ${destinationLabel(destination)}`"
            :title="`Forget ${destinationLabel(destination)}`"
            :aria-disabled="recentActionsDisabled || undefined"
            @click="
              (event: MouseEvent) => forgetDestination(destination, event)
            "
          >
            <UiIcon name="cross" />
          </UiIconButton>
        </li>
      </ul>
      <p
        v-if="mode === 'add' && forgetError"
        class="remote-dialog-error"
        role="alert"
      >
        {{ forgetError }}
      </p>
    </form>

    <div
      v-else
      class="remote-directory-dialog"
      @keydown="handleDirectorySurfaceKeydown"
    >
      <UiContextMenu
        :items="() => textFieldItems(() => directoryFilterInput?.input)"
      >
        <UiInput
          ref="directoryFilterInput"
          v-model="directoryFilter"
          class="remote-directory-filter"
          variant="mono"
          :error="Boolean(connectionError)"
          type="text"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="Filter directories"
          aria-label="Filter Remote Directories"
          aria-controls="remote-directory-list"
          :aria-activedescendant="`remote-directory-option-${selectedIndex}`"
          :aria-describedby="
            connectionError ? 'remote-directory-error' : undefined
          "
          :aria-invalid="Boolean(connectionError)"
          :readonly="connecting"
          @keydown="handleDirectoryKeydown"
        />
      </UiContextMenu>
      <p
        v-if="connectionError"
        id="remote-directory-error"
        class="remote-dialog-error"
        role="alert"
      >
        {{ connectionError }}
      </p>
      <div
        id="remote-directory-list"
        class="remote-directory-list"
        role="listbox"
      >
        <UiListOption
          v-for="(option, index) in directoryOptions"
          :id="`remote-directory-option-${index}`"
          :key="option.path"
          class="remote-directory-option ui-pick-row"
          tabindex="-1"
          :data-cursor="index === selectedIndex || undefined"
          :aria-selected="index === selectedIndex"
          :disabled="connecting"
          :title="option.path"
          @mousedown.prevent
          @mouseenter="() => highlight(index)"
          @click="() => chooseDirectory(option)"
        >
          {{ option.name }}
        </UiListOption>
      </div>
    </div>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

import type { SshDestination } from '../composables/state';
import textFieldItems from '../lib/text-menu';

import UiContextMenu from './ui/UiContextMenu.vue';
import UiDialog from './ui/UiDialog.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiInput from './ui/UiInput.vue';
import UiListOption from './ui/UiListOption.vue';

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

const props = withDefaults(
  defineProps<{
    step: 'connection' | 'directory';
    mode: 'add' | 'retry';
    connectionError: string;
    connecting: boolean;
    directoryOptions: RemoteDirectoryOption[];
    recentDestinations?: SshDestination[];
    forgetPending?: boolean;
    forgetError?: string;
    returnFocus?: () => HTMLElement | undefined;
  }>(),
  {
    recentDestinations: () => [],
    forgetError: '',
    returnFocus: undefined,
  },
);

const emit = defineEmits<{
  'submit-connection': [];
  'choose-directory': [path: string, kind: 'back' | 'select' | 'forward'];
  'select-destination': [destination: SshDestination];
  'forget-destination': [destination: SshDestination];
}>();

const connectionInput = ref<InstanceType<typeof UiInput>>();
const directoryFilterInput = ref<InstanceType<typeof UiInput>>();
const recentList = ref<HTMLUListElement>();
const recentActionsDisabled = computed(
  () => props.connecting || props.forgetPending,
);
let forgetOrigin: HTMLButtonElement | undefined;

watch(
  () => [open.value, props.mode, props.step],
  () => {
    forgetOrigin = undefined;
  },
  { flush: 'sync' },
);

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

watch(
  () => props.recentDestinations,
  (destinations, previous) => {
    const origin = forgetOrigin;
    forgetOrigin = undefined;
    if (!origin || origin !== document.activeElement) return;
    const buttons = recentList.value?.querySelectorAll<HTMLButtonElement>(
      '.remote-recent-forget',
    );
    if (!buttons) return;
    const index = Array.from(buttons).indexOf(origin);
    const removed = previous[index];
    const remaining = new Set(destinations.map(destinationLabel));
    if (!removed || remaining.has(destinationLabel(removed))) return;

    // Move focus before the dialog's focus trap sees the active row disappear.
    let next: HTMLButtonElement | undefined;
    for (const [i, destination] of previous.entries()) {
      if (!remaining.has(destinationLabel(destination))) continue;
      next = buttons[i];
      if (i > index) break;
    }
    (next ?? connectionInput.value?.input)?.focus();
  },
);

function destinationLabel({ host, user, port }: SshDestination): string {
  return `${user ? `${user}@` : ''}${host}${port === undefined ? '' : ` -p ${port}`}`;
}

function selectDestination(destination: SshDestination): void {
  if (!recentActionsDisabled.value) emit('select-destination', destination);
}

function forgetDestination(
  destination: SshDestination,
  event: MouseEvent,
): void {
  if (recentActionsDisabled.value) return;
  if (event.currentTarget instanceof HTMLButtonElement) {
    forgetOrigin = event.currentTarget;
    forgetOrigin.focus({ preventScroll: true });
  }
  emit('forget-destination', destination);
}

function submitConnection(): void {
  if (!props.connecting && !(props.mode === 'add' && props.forgetPending)) {
    emit('submit-connection');
  }
}

function highlight(index: number): void {
  selectedIndex.value = index;
}

function chooseDirectory(option: RemoteDirectoryOption): void {
  emit('choose-directory', option.path, option.kind);
}

function handleConnectionKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape' && props.connecting) {
    event.preventDefault();
    event.stopPropagation();
    return;
  }
  if (
    event.key === 'Enter' &&
    (event.metaKey || event.ctrlKey) &&
    !event.isComposing
  ) {
    event.preventDefault();
    if (event.target === connectionInput.value?.input) submitConnection();
  }
  handleRecentKeys(event);
}

function handleRecentKeys(event: KeyboardEvent): void {
  if (
    !recentList.value ||
    recentActionsDisabled.value ||
    event.isComposing ||
    event.altKey ||
    event.metaKey ||
    event.ctrlKey ||
    event.shiftKey ||
    (event.key !== 'ArrowDown' && event.key !== 'ArrowUp')
  ) {
    return;
  }
  const input = connectionInput.value?.input;
  const rows = Array.from(
    recentList.value.querySelectorAll<HTMLButtonElement>(
      '.remote-recent-destination',
    ),
  );
  const index = rows.findIndex((row) => row === event.target);
  if (index < 0 && event.target !== input) return;
  const next = event.key === 'ArrowDown' ? index + 1 : index - 1;
  if (next < -1) return;
  event.preventDefault();
  if (next === -1) input?.focus();
  else rows[next]?.focus();
}

function handleDirectorySurfaceKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape' && props.connecting) {
    event.preventDefault();
    event.stopPropagation();
    return;
  }
  if (!event.metaKey && !event.ctrlKey) return;

  if (event.key === 'ArrowUp') {
    event.preventDefault();
    if (props.connecting) return;
    const back = props.directoryOptions.find(
      (option) => option.kind === 'back',
    );
    if (back) chooseDirectory(back);
    return;
  }
  if (event.key === 'Enter' && !event.isComposing) {
    event.preventDefault();
    if (props.connecting) return;
    const select = props.directoryOptions.find(
      (option) => option.kind === 'select',
    );
    if (select) chooseDirectory(select);
  }
}

function handleDirectoryKeydown(event: KeyboardEvent): void {
  if (props.connecting || event.metaKey || event.ctrlKey) return;
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
.remote-connection-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.remote-directory-dialog {
  display: contents;
}

.remote-directory-filter {
  flex: none;
}

.remote-dialog-error {
  margin: 0;
  color: var(--danger);
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
}

.remote-recent-list {
  max-height: min(160px, 28vh);
  margin: 0;
  padding: 1px;
  overflow: auto;
  overscroll-behavior: contain;
  list-style: none;
}

.remote-recent-row {
  display: flex;
  border-radius: var(--radius-md);
  gap: 4px;
}

.remote-recent-row:hover {
  background: var(--hover);
}

.remote-recent-row:has(:focus-visible) {
  background: var(--selected);
}

.remote-recent-destination {
  flex: 1;
  min-width: 0;
  height: var(--control-lg);
  padding: 0 8px;
  overflow: hidden;
  background: transparent;
  color: var(--text);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-sm);
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remote-recent-forget :deep(svg) {
  font-size: 11px;
}

.remote-recent-destination:is(:hover, :focus-visible) {
  background: transparent;
}

.remote-recent-destination:focus-visible {
  outline: none;
}

.remote-recent-forget:focus-visible {
  box-shadow: none;
}

.remote-recent-row [aria-disabled='true'] {
  opacity: 0.5;
}

.remote-directory-list {
  max-height: min(360px, calc(100vh - 150px));
  overflow-y: auto;
  overscroll-behavior: contain;
}

.remote-directory-option {
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
</style>
