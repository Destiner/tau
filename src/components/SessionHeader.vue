<template>
  <header
    ref="header"
    class="session-header"
  >
    <div class="session-heading">
      <UiTooltip
        v-if="projectTrustShield"
        text="Untrusted Project"
        side="bottom"
      >
        <UiIconButton
          class="project-trust-shield"
          size="sm"
          variant="fill"
          label="Untrusted Project"
          @click="openProjectTrust"
        >
          <svg
            aria-hidden="true"
            viewBox="0 0 20 20"
            fill="none"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path
              d="M10 2.5 16.5 5v4.4c0 3.6-2.6 6.2-6.5 8.1-3.9-1.9-6.5-4.5-6.5-8.1V5L10 2.5Z"
            />
            <path d="M10 6.5v3.7m0 2.6h.01" />
          </svg>
        </UiIconButton>
      </UiTooltip>
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
        <UiTooltip
          v-if="canRenameSession"
          text="Rename Session"
          shortcut="Mod+Shift+R"
          side="bottom"
        >
          <button
            class="session-name"
            type="button"
            @click="beginSessionRename"
          >
            {{ sessionTitle }}
          </button>
        </UiTooltip>
        <span
          v-else
          class="session-name"
          >{{ sessionTitle }}</span
        >
      </h1>
    </div>
    <UiTooltip
      v-if="activeProject"
      text="New Session"
      shortcut="Mod+N"
      side="bottom"
    >
      <UiIconButton
        class="session-header-action"
        size="lg"
        label="New Session"
        :disabled="projectActionsDisabled"
        @click="handleNewSession"
      >
        <UiIcon name="plus" />
      </UiIconButton>
    </UiTooltip>
    <UiTooltip
      text="Command Palette"
      shortcut="Mod+K"
      side="bottom"
    >
      <UiIconButton
        class="session-header-action session-palette-button"
        size="lg"
        label="Open Command Palette"
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
    </UiTooltip>
  </header>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

import useTau from '../composables/useTau';

import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiInput from './ui/UiInput.vue';
import UiTooltip from './ui/UiTooltip.vue';

const props = withDefaults(
  defineProps<{
    /** Keeps an in-progress rename open while the palette takes focus. */
    paletteOpen?: boolean;
    projectTrustShield?: boolean;
  }>(),
  { paletteOpen: false, projectTrustShield: false },
);

const emit = defineEmits<{
  'composer-focus': [];
  'palette-toggle': [];
  'new-session': [];
  'project-trust': [trigger: HTMLButtonElement];
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

function openProjectTrust(event: MouseEvent): void {
  if (event.currentTarget instanceof HTMLButtonElement)
    emit('project-trust', event.currentTarget);
}

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
  display: flex;
  flex: 1;
  align-items: center;
  gap: 3px;
  min-width: 0;
  margin-left: -5px;
}

.project-trust-shield[data-size='sm'] {
  width: 22px;
  height: 22px;
}

.project-trust-shield svg {
  width: 15px;
  height: 15px;
  stroke: light-dark(#8a5000, #ffbc55);
}

.session-heading h1 {
  min-width: 0;
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
