<template>
  <main class="fixture-shell">
    <TranscriptView
      class="fixture-transcript"
      :messages="messages"
      :compacting="false"
      :show-working-indicator="false"
      working-label="Working"
      session-key="extension-dialog-fixture"
      :copy-paths="remote"
      :prompt="promptVisible && !answered ? prompt : undefined"
      :prompt-disabled="disabled"
      @prompt-submit="handleSubmit"
      @prompt-cancel="handleCancel"
      @prompt-draft="updateDraft"
    />
    <button
      v-if="delayed && !promptVisible"
      class="show-prompt"
      type="button"
      @click="showPrompt"
    >
      Show prompt
    </button>
    <output data-testid="dialog-outcome">{{ outcome }}</output>
  </main>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue';

import TranscriptView from '../components/TranscriptView.vue';
import type { ExtensionDialog } from '../composables/state';
import type { TranscriptEntry } from '../lib/pi/transcript';

const params = new URLSearchParams(window.location.search);
const remote = params.get('remote') === 'true';
const delayed = params.get('delayed') === 'true';
const requestedMethod = params.get('method');
const method = isPromptMethod(requestedMethod) ? requestedMethod : 'select';
const submitting = params.get('submitting') === 'true';
const disabled = params.get('disabled') === 'true';

const title = [
  'Plan /home/agent/.pi/workflows/implement/RHI-6283/implementation-plan.md',
  '',
  'Which label should the pull request carry?',
  '',
  'Preview:',
  '```ts',
  "const label = 'release';",
  '```',
].join('\n');

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
  method,
  title,
  message,
  options:
    method === 'select'
      ? Array.from({ length: 12 }, (_, index) => `label-${index}`)
      : undefined,
  draft: params.get('draft') ?? '',
  submitting,
  error: params.get('error') ?? '',
  controllerKey: 'fixture-controller',
  runtimeId: 'fixture-runtime',
  generation: 1,
  projectName: 'tau',
  sessionName: 'Long question',
  workingDirectory: '/home/agent/rhinestone',
});

const promptVisible = ref(!delayed);
const answered = ref(false);
const outcome = ref('');
const cancelCount = ref(0);

function showPrompt(): void {
  promptVisible.value = true;
}

function updateDraft(value: string): void {
  prompt.draft = value;
}

function handleSubmit(value: string | boolean): void {
  answered.value = true;
  outcome.value = JSON.stringify({ submit: value });
}

function handleCancel(): void {
  cancelCount.value += 1;
  answered.value = true;
  outcome.value = JSON.stringify({ cancel: true, count: cancelCount.value });
}

function isPromptMethod(
  value: string | null,
): value is ExtensionDialog['method'] {
  return (
    value === 'select' ||
    value === 'confirm' ||
    value === 'input' ||
    value === 'editor'
  );
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

.show-prompt,
.fixture-shell output {
  position: absolute;
  top: 8px;
  left: 8px;
  color: var(--text);
}

.show-prompt {
  z-index: 1;
}
</style>
