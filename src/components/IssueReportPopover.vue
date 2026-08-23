<template>
  <!--
    The tooltip is outside `PopoverRoot`, not around the trigger: a trigger
    registers with the nearest popper root, and from inside the tooltip that
    would be the tooltip's own.
  -->
  <UiTooltip text="Report an Issue">
    <span class="issue-report-trigger">
      <PopoverRoot v-model:open="open">
        <PopoverTrigger as-child>
          <UiIconButton
            size="lg"
            label="Report an Issue"
          >
            <UiIcon name="bug" />
          </UiIconButton>
        </PopoverTrigger>
        <PopoverPortal>
          <PopoverContent
            class="issue-report-popover ui-surface"
            side="top"
            align="end"
            :side-offset="5"
            aria-label="Report an Issue"
            :aria-busy="submitting || undefined"
          >
            <form
              class="issue-report-form"
              @submit.prevent="handleSubmit"
            >
              <UiCheckbox
                v-model="includeCurrentSession"
                :disabled="!sessionId || submitting"
                >Include Current Session</UiCheckbox
              >

              <label class="description-field">
                <span>Describe the Issue</span>
                <UiContextMenu
                  :items="() => textFieldItems(() => descriptionInput?.input)"
                >
                  <UiTextarea
                    ref="descriptionInput"
                    v-model="description"
                    rows="6"
                    maxlength="10000"
                    :readonly="submitting"
                    aria-label="Describe the Issue"
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
    </span>
  </UiTooltip>
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
import UiCheckbox from './ui/UiCheckbox.vue';
import UiContextMenu from './ui/UiContextMenu.vue';
import UiIcon from './ui/UiIcon.vue';
import UiIconButton from './ui/UiIconButton.vue';
import UiTextarea from './ui/UiTextarea.vue';
import UiTooltip from './ui/UiTooltip.vue';

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
/* Only there to be something the tooltip can point at; it is the button. */
.issue-report-trigger {
  display: flex;
}

:global(.issue-report-popover) {
  z-index: 20;
  width: min(280px, calc(100vw - 18px));
  padding: 10px;
  outline: 0;
}

.issue-report-form {
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.description-field {
  display: flex;
  flex-direction: column;
  color: var(--muted);
  font-size: var(--text-xs);
  gap: 5px;
}

.description-field :deep(.ui-textarea) {
  min-height: 108px;
  resize: none;
}

.issue-report-error {
  margin: 0;
  color: var(--danger);
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
}

.issue-report-actions {
  display: flex;
  justify-content: flex-end;
}
</style>
