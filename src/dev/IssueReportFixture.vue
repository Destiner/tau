<template>
  <main class="fixture-shell">
    <IssueReportPopover
      session-id="fixture-session"
      :api="api"
    />
    <output data-testid="submitted-report">{{ submittedReport }}</output>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import IssueReportPopover from '../components/IssueReportPopover.vue';

import { createCloudFixture } from './cloud-fixture';

const submittedReport = ref('');
const search = new URLSearchParams(window.location.search);
const api = createCloudFixture({
  configured: search.get('setup') !== 'true',
  delay: search.get('slow') === 'true' ? 350 : 0,
  onReport(report) {
    submittedReport.value = JSON.stringify({
      description: report.description,
      ...(report.sessionId ? { sessionId: report.sessionId } : {}),
    });
  },
});
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
