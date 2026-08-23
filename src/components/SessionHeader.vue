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
        @blur="commitSessionRename"
        @keydown.enter.prevent="commitSessionRename"
        @keydown.escape.prevent="cancelSessionRename"
      />
      <h1 v-else>
        <UiTooltip
          v-if="canRenameSession"
          text="Rename Session"
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
      side="bottom"
    >
      <UiIconButton
        class="session-new-button"
        size="lg"
        label="New Session"
        :disabled="projectActionsDisabled"
        @click="handleNewSession"
      >
        <UiIcon name="plus" />
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

const emit = defineEmits<{ 'composer-focus': [] }>();

const {
  activeProject,
  canRenameSession,
  projectActionsDisabled,
  newSession,
  renameSession,
  sessionTitle,
} = useTau();

const header = ref<HTMLElement>();
const sessionTitleInput = ref<InstanceType<typeof UiInput>>();
const renamingSession = ref(false);
const sessionNameDraft = ref('');

watch(canRenameSession, (renamable) => {
  if (!renamable) cancelSessionRename();
});

function handleNewSession(): void {
  if (activeProject.value) void newSession(activeProject.value);
}

function beginSessionRename(): void {
  if (!canRenameSession.value) return;
  sessionNameDraft.value = sessionTitle.value;
  renamingSession.value = true;
  void nextTick(() => {
    sessionTitleInput.value?.input?.focus();
    sessionTitleInput.value?.input?.select();
  });
}

function commitSessionRename(): void {
  // Escape and a lost runtime both close the field before its blur arrives,
  // and neither should apply the name that was left in it.
  if (!renamingSession.value) return;
  closeSessionRename();
  void renameSession(sessionNameDraft.value);
}

function cancelSessionRename(): void {
  if (!renamingSession.value) return;
  closeSessionRename();
}

/**
 * Committing on blur means focus has already moved on, so the composer is only
 * refocused when the field itself still holds focus, as it does after Enter,
 * Escape, or a runtime that stopped mid-rename.
 */
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
  cancelRename: cancelSessionRename,
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

.session-new-button {
  flex: none;
  width: 24px;
  height: 24px;
  padding: 4px;
  color: var(--muted);
}

.session-heading {
  flex: 1;
  min-width: 0;

  /* Offsets the padding the name carries so it lines up with the transcript. */
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
