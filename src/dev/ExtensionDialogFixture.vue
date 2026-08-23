<template>
  <main class="fixture-shell">
    <TranscriptView
      class="fixture-transcript"
      :messages="messages"
      :show-working-indicator="false"
      working-label="Working"
      session-key="extension-dialog-fixture"
      :prompt="answered ? undefined : prompt"
      @prompt-submit="handleSubmit"
      @prompt-cancel="handleCancel"
      @prompt-draft="updateDraft"
    />
    <output data-testid="dialog-outcome">{{ outcome }}</output>
  </main>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue';

import TranscriptView from '../components/TranscriptView.vue';
import type { ExtensionDialog } from '../composables/state';
import type { TranscriptEntry } from '../lib/pi/transcript';

const title = 'Which label should the pull request carry?';

/** Long enough, and wide enough, to overflow the pane on any window. */
const message = [
  '| Label | Meaning | Used by |',
  '| --- | --- | --- |',
  // An unbreakable value is what makes a table wider than the prompt holding it.
  `| label-wide | ${'0123456789abcdef'.repeat(8)} | team-wide |`,
  ...Array.from(
    { length: 40 },
    (_, index) => `| label-${index} | Reason number ${index} | team-${index} |`,
  ),
].join('\n');

/** History above the prompt, so the prompt is reached by scrolling to it. */
const messages = ref<TranscriptEntry[]>(
  Array.from({ length: 8 }, (_, index) => index).flatMap((index) => [
    { id: `user-${index}`, kind: 'user', text: `History prompt ${index}` },
    {
      id: `assistant-${index}`,
      kind: 'assistant',
      text: `Reply ${index}. Something the reader saw before the question.`,
    },
  ]),
);

const prompt = reactive<ExtensionDialog>({
  key: 'fixture-prompt',
  requestId: 'fixture-request',
  method: 'select',
  title,
  message,
  options: Array.from({ length: 12 }, (_, index) => `label-${index}`),
  draft: '',
  submitting: false,
  error: '',
  controllerKey: 'fixture-controller',
  runtimeId: 'fixture-runtime',
  generation: 1,
  projectName: 'tau',
  sessionName: 'Long question',
});

const answered = ref(false);
const outcome = ref('');

function updateDraft(value: string): void {
  prompt.draft = value;
}

function handleSubmit(value: string | boolean): void {
  answered.value = true;
  outcome.value = JSON.stringify({ submit: value });
}

function handleCancel(): void {
  answered.value = true;
  outcome.value = JSON.stringify({ cancel: true });
}
</script>

<style scoped>
.fixture-shell {
  display: grid;
  grid-template-rows: minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

.fixture-transcript {
  min-height: 0;
}

.fixture-shell output {
  position: absolute;
  top: 8px;
  left: 8px;
  color: var(--text);
}
</style>
