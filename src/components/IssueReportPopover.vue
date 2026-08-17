<template>
  <PopoverRoot v-model:open="open">
    <PopoverTrigger as-child>
      <UiIconButton
        size="lg"
        label="Report an issue"
        title="Report an issue"
      >
        <UiIcon name="bug" />
      </UiIconButton>
    </PopoverTrigger>
    <PopoverPortal>
      <PopoverContent
        class="issue-report-popover"
        side="top"
        align="end"
        :side-offset="5"
        aria-label="Report an issue"
        :aria-busy="submitting || undefined"
      >
        <form
          class="issue-report-form"
          @submit.prevent="handleSubmit"
        >
          <label class="session-option">
            <input
              v-model="includeCurrentSession"
              type="checkbox"
              :disabled="!sessionId || submitting"
            />
            <span>Include current session</span>
          </label>

          <label class="description-field">
            <span>Describe the issue</span>
            <UiContextMenu
              :items="() => textFieldItems(() => descriptionInput?.input)"
            >
              <UiTextarea
                ref="descriptionInput"
                v-model="description"
                rows="6"
                maxlength="10000"
                :readonly="submitting"
                aria-label="Describe the issue"
              />
            </UiContextMenu>
          </label>

          <p
            v-if="error"
            class="issue-report-error"
            role="alert"
          >
            {{ error }}
          </p>

          <footer class="issue-report-actions">
            <UiButton
              variant="primary"
              type="submit"
              :disabled="submitting || !description.trim()"
            >
              Submit
            </UiButton>
          </footer>
        </form>
      </PopoverContent>
    </PopoverPortal>
  </PopoverRoot>
</template>

<script setup lang="ts">
import {
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from 'reka-ui';
import { ref, watch } from 'vue';

import textFieldItems from '../lib/text-menu';

import UiButton from './ui/UiButton.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiTextarea from './ui/UiTextarea.vue';

const props = defineProps<{
  sessionId?: string;
  submitReport: (description: string, sessionId?: string) => Promise<void>;
}>();

const open = ref(false);
const description = ref('');
const includeCurrentSession = ref(false);
const submitting = ref(false);
const error = ref('');
const descriptionInput = ref<InstanceType<typeof UiTextarea>>();

watch(description, () => {
  error.value = '';
});

async function handleSubmit(): Promise<void> {
  const submittedDescription = description.value.trim();
  if (!submittedDescription || submitting.value) return;

  submitting.value = true;
  error.value = '';
  try {
    await props.submitReport(
      submittedDescription,
      includeCurrentSession.value ? props.sessionId : undefined,
    );
    description.value = '';
    includeCurrentSession.value = false;
    open.value = false;
  } catch {
    error.value = 'The report could not be saved. Try again.';
  } finally {
    submitting.value = false;
  }
}
</script>

<style scoped>
:global(.issue-report-popover) {
  z-index: 20;
  width: min(280px, calc(100vw - 18px));
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  outline: 0;
  background: var(--panel-raised);
  box-shadow: 0 8px 24px var(--shadow-soft);
}

.issue-report-form {
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.session-option {
  display: flex;
  align-items: center;
  color: var(--muted);
  font-size: 11px;
  gap: 6px;
}

.session-option input {
  width: 13px;
  height: 13px;
  margin: 0;
}

.description-field {
  display: flex;
  flex-direction: column;
  color: var(--text);
  font-size: 11px;
  gap: 5px;
}

.description-field :deep(.ui-textarea) {
  min-height: 108px;
  resize: none;
}

.issue-report-error {
  margin: 0;
  color: var(--danger);
  font-size: 11px;
  line-height: 1.4;
}

.issue-report-actions {
  display: flex;
  justify-content: flex-end;
}
</style>
