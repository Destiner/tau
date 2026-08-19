<template>
  <main class="fixture-shell">
    <footer class="composer-area">
      <ExtensionDialog
        v-model:draft="draft"
        method="select"
        :title="title"
        :message="message"
        :options="options"
        project-name="tau"
        session-name="Long question"
        @submit="handleSubmit"
        @cancel="handleCancel"
      />
    </footer>
    <output data-testid="dialog-outcome">{{ outcome }}</output>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import ExtensionDialog from '../components/ExtensionDialog.vue';

const title = 'Which label should the pull request carry?';

/** Long enough, and wide enough, to overflow the composer on any window. */
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

const options = Array.from({ length: 12 }, (_, index) => `label-${index}`);

const draft = ref('');
const outcome = ref('');

function handleSubmit(value: string | boolean): void {
  outcome.value = JSON.stringify({ submit: value });
}

function handleCancel(): void {
  outcome.value = JSON.stringify({ cancel: true });
}
</script>

<style scoped>
.fixture-shell {
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  width: 100%;
  height: 100%;
  background: var(--panel);
}

.composer-area {
  padding: 6px 8px 8px 6px;
  border-top: 1px solid var(--border);
  background: var(--canvas);
}

.fixture-shell output {
  position: absolute;
  top: 8px;
  left: 8px;
  color: var(--text);
}
</style>
