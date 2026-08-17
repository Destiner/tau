<template>
  <main class="fixture-shell">
    <IssueReportPopover
      session-id="fixture-session"
      :submit-report="submitReport"
    />
    <output data-testid="submitted-report">{{ submittedReport }}</output>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import IssueReportPopover from '../components/IssueReportPopover.vue';

const submittedReport = ref('');

async function submitReport(
  description: string,
  sessionId?: string,
): Promise<void> {
  if (description === 'fail') throw new Error('Fixture failure');
  submittedReport.value = JSON.stringify({ description, sessionId });
}
</script>

<style scoped>
.fixture-shell {
  display: flex;
  align-items: flex-end;
  justify-content: flex-end;
  width: 100%;
  height: 100%;
  padding: 6px;
  background: var(--panel);
}

.fixture-shell output {
  position: absolute;
  top: 8px;
  left: 8px;
  color: var(--text);
}
</style>
