<template>
  <header
    ref="header"
    class="session-header"
  >
    <div class="session-heading">
      <UiInput
        v-if="renamingSession"
        ref="sessionTitleInput"
        v-model="sessionNameDraft"
        class="session-name-input"
        variant="bare"
        type="text"
        maxlength="240"
        spellcheck="false"
        aria-label="Session Name"
        @blur="handleSessionRenameBlur"
        @keydown.enter.prevent="commitSessionRename"
        @keydown.escape.prevent="cancelSessionRename"
      />
      <h1 v-else>
        <UiButton
          v-if="canRenameSession"
          class="session-name"
          variant="plain"
          :label="sessionTitle"
          :tooltip="{
            text: 'Rename Session',
            shortcut: 'Mod+Shift+R',
            side: 'bottom',
          }"
          @click="beginSessionRename"
        />
        <span
          v-else
          class="session-name"
          >{{ sessionTitle }}</span
        >
      </h1>
    </div>
    <UiIconButton
      v-if="activeProject"
      class="session-header-action"
      size="lg"
      label="New Session"
      :tooltip="{ shortcut: 'Mod+N', side: 'bottom' }"
      :disabled="projectActionsDisabled"
      @click="handleNewSession"
    >
      <UiIcon name="plus" />
    </UiIconButton>
    <UiIconButton
      class="session-header-action session-palette-button"
      size="lg"
      label="Open Command Palette"
      :tooltip="{ text: 'Command Palette', shortcut: 'Mod+K', side: 'bottom' }"
      @pointerdown.prevent
      @click="handlePaletteToggle"
    >
      <svg
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
    </UiIconButton>
  </header>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

import useTau from '../composables/useTau';

import UiButton from './ui/UiButton.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiInput from './ui/UiInput.vue';

const props = withDefaults(
  defineProps<{
    /** Keeps an in-progress rename open while the palette takes focus. */
    paletteOpen?: boolean;
  }>(),
  { paletteOpen: false },
);

const emit = defineEmits<{
  'composer-focus': [];
  'palette-toggle': [];
  'new-session': [];
}>();

const {
  activeProject,
  canRenameSession,
  projectActionsDisabled,
  renameSession,
  sessionTitle,
} = useTau();

const header = ref<HTMLElement>();
const sessionTitleInput = ref<InstanceType<typeof UiInput>>();
const renamingSession = ref(false);
const paletteFocusPending = ref(false);
const sessionNameDraft = ref('');

watch(
  () => props.paletteOpen,
  (open) => {
    if (!open) paletteFocusPending.value = false;
  },
);

watch(canRenameSession, (renamable) => {
  if (!renamable) cancelSessionRename();
});

function handleNewSession(): void {
  emit('new-session');
}

function handlePaletteToggle(): void {
  emit('palette-toggle');
}

function beginSessionRename(): void {
  if (!canRenameSession.value || renamingSession.value) return;
  sessionNameDraft.value = sessionTitle.value;
  renamingSession.value = true;
  void nextTick(() => {
    sessionTitleInput.value?.input?.focus();
    sessionTitleInput.value?.input?.select();
  });
}

function handleSessionRenameBlur(): void {
  if (props.paletteOpen || paletteFocusPending.value) return;
  commitSessionRename();
}

function commitSessionRename(): void {
  if (!renamingSession.value) return;
  closeSessionRename();
  void renameSession(sessionNameDraft.value);
}

function cancelSessionRename(): void {
  if (!renamingSession.value) return;
  closeSessionRename();
}

function closeSessionRename(): void {
  const focused = document.activeElement === sessionTitleInput.value?.input;
  renamingSession.value = false;
  if (focused) void nextTick(() => emitComposerFocus());
}

function emitComposerFocus(): void {
  emit('composer-focus');
}

/** App-level surfaces call this when a rename cannot outlive them. */
defineExpose({
  beginRename: beginSessionRename,
  preparePaletteOpen: () => {
    paletteFocusPending.value = true;
  },
  cancelRename: cancelSessionRename,
  get renaming() {
    return renamingSession.value;
  },
  get header() {
    return header.value;
  },
});
</script>

<style scoped>
.session-header {
  display: flex;
  flex: none;
  align-items: center;
  height: 30px;
  min-height: 30px;
  padding: 0 8px 0 12px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--canvas) 92%, transparent);
  gap: 6px;
}

.session-header-action {
  flex: none;
  width: 24px;
  height: 24px;
  padding: 4px;
  color: var(--muted);
}

.session-palette-button svg {
  width: 16px;
  height: 16px;
}

.session-heading {
  flex: 1;
  min-width: 0;
  margin-left: -5px;
}

.session-heading h1 {
  max-width: 60ch;
  margin: 0;
  overflow: hidden;
  color: var(--text);
  font-size: 13px;
  font-weight: 400;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-name {
  display: block;
  max-width: 100%;
  padding: 2px 5px;
  overflow: hidden;
  border-radius: 5px;
  background: transparent;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

button.session-name {
  cursor: text;
}

button.session-name:hover,
button.session-name:focus-visible {
  background: var(--hover);
}
</style>
