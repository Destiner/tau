<template>
  <UiDialog
    :open="state.projectRename !== null"
    title="Rename Project"
    width="compact"
    :busy="Boolean(rename?.busy && !rename.timedOut)"
    :return-focus="returnFocus"
    @update:open="cancelProjectRename"
  >
    <UiInput
      ref="nameInput"
      :model-value="rename?.draft ?? ''"
      aria-label="Project Name"
      :aria-invalid="Boolean(rename?.error)"
      :aria-describedby="rename?.error ? 'project-rename-error' : undefined"
      :maxlength="240"
      :disabled="rename?.busy"
      :error="Boolean(rename?.error)"
      @update:model-value="updateDraft"
      @keydown.enter="submit"
    />
    <p
      v-if="rename?.error"
      id="project-rename-error"
      role="alert"
      class="rename-error"
    >
      {{ rename.error }}
    </p>
    <template #footer>
      <UiButton
        :disabled="rename?.busy && !rename.timedOut"
        @click="cancelProjectRename"
        >Cancel</UiButton
      >
      <UiButton
        variant="primary"
        :disabled="rename?.busy"
        @click="confirmProjectRename"
        >{{ delayedBusy ? 'Renaming…' : 'Rename' }}</UiButton
      >
    </template>
  </UiDialog>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import { state } from '../composables/state';
import useTau from '../composables/useTau';

import UiButton from './ui/UiButton.vue';
import UiDialog from './ui/UiDialog.vue';
import UiInput from './ui/UiInput.vue';

defineProps<{ returnFocus: () => HTMLElement | undefined }>();
const { cancelProjectRename, confirmProjectRename } = useTau();
const rename = computed(() => state.projectRename);
const nameInput = ref<InstanceType<typeof UiInput>>();
const delayedBusy = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;
watch(
  () => rename.value?.busy,
  (busy) => {
    clearTimeout(timer);
    delayedBusy.value = false;
    if (busy)
      timer = setTimeout(() => {
        delayedBusy.value = true;
      }, 300);
  },
);
watch(
  () => rename.value?.path,
  async (path) => {
    if (!path) return;
    await nextTick();
    nameInput.value?.input?.focus();
    nameInput.value?.input?.select();
  },
);
onBeforeUnmount(() => clearTimeout(timer));
function updateDraft(value: string): void {
  if (rename.value && !rename.value.busy) rename.value.draft = value;
}
function submit(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  void confirmProjectRename();
}
</script>

<style scoped>
.rename-error {
  margin: 0;
  color: var(--danger);
  font-size: var(--text-sm);
  line-height: var(--leading-ui);
}
</style>
