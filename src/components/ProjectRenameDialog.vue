<template>
  <UiDialog
    :open="rename !== null"
    title="Rename Project"
    :description="location"
    class="project-rename"
    data-modal-size="compact"
    :busy="Boolean(rename?.saving && !rename.unconfirmed)"
    :return-focus="returnFocus"
    @update:open="closeProjectRename"
    @open-auto-focus="focusName"
    @keydown.enter="submit"
  >
    <label for="project-rename-name">Display Name</label>
    <UiContextMenu :items="() => textFieldItems(() => nameInput?.input)">
      <UiInput
        id="project-rename-name"
        ref="nameInput"
        :model-value="rename?.name ?? ''"
        aria-label="Project Name"
        :aria-invalid="Boolean(rename?.error && !rename.unconfirmed)"
        :aria-describedby="rename?.error ? 'project-rename-error' : undefined"
        :maxlength="240"
        :disabled="rename?.saving"
        :error="Boolean(rename?.error && !rename.unconfirmed)"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        @update:model-value="updateDraft"
      />
    </UiContextMenu>
    <div class="rename-feedback">
      <p
        v-if="rename?.error"
        id="project-rename-error"
        role="alert"
        :class="{ 'rename-error': !rename.unconfirmed }"
      >
        {{ rename.error }}
      </p>
    </div>
    <template #footer>
      <UiButton
        :disabled="rename?.saving && !rename.unconfirmed"
        @click="closeProjectRename"
        >{{ rename?.unconfirmed ? 'Close' : 'Cancel' }}</UiButton
      >
      <UiButton
        class="rename-submit"
        variant="primary"
        :disabled="rename?.saving"
        @click="submitProjectRename"
        >{{ submitLabel }}</UiButton
      >
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { state } from '../composables/state';
import useTau from '../composables/useTau';
import textFieldItems from '../lib/text-menu';

import UiButton from './ui/UiButton.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiDialog from './ui/UiDialog.vue';
import UiInput from './ui/UiInput.vue';

defineProps<{ returnFocus: () => HTMLElement | undefined }>();
const { closeProjectRename, submitProjectRename } = useTau();
const rename = computed(() => state.projectRename);
const nameInput = ref<InstanceType<typeof UiInput>>();
const location = computed(() => {
  const project = state.workspace?.projects.find(
    (item) => item.path === rename.value?.projectPath,
  );
  return project
    ? [project.connectionString, project.workingDirectory]
        .filter(Boolean)
        .join(' · ')
    : undefined;
});
const submitLabel = computed(() => {
  if (rename.value?.saving && rename.value.showProgress) return 'Renaming…';
  return rename.value?.error && !rename.value.unconfirmed ? 'Retry' : 'Rename';
});

function focusName(event: Event): void {
  const input = nameInput.value?.input;
  if (!input || input.disabled) return;
  event.preventDefault();
  input.focus();
  input.select();
}

function updateDraft(value: string): void {
  if (!rename.value || rename.value.saving) return;
  rename.value.name = value;
  rename.value.error = '';
}

function submit(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return;
  if (
    !(event.target instanceof HTMLInputElement) &&
    !event.metaKey &&
    !event.ctrlKey
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  void submitProjectRename();
}
</script>

<style scoped>
label {
  color: var(--muted);
  font-size: var(--text-sm);
}

.rename-feedback {
  min-height: 3lh;
  color: var(--muted);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}

.rename-feedback p {
  margin: 0;
}

.rename-error {
  color: var(--danger);
}

.rename-submit {
  min-width: 88px;
}
</style>
