<template>
  <main class="fixture-shell">
    <RemoteDialog
      v-model:open="open"
      v-model:connection-string="connectionString"
      v-model:directory-filter="directoryFilter"
      v-model:selected-index="selectedIndex"
      :step="step"
      mode="add"
      :connection-error="connectionError"
      :connecting="connecting"
      :directory-options="directoryOptions"
      @submit-connection="showDirectories"
      @choose-directory="showDirectories"
    />
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import RemoteDialog from '../components/RemoteDialog.vue';

const search = new URLSearchParams(window.location.search);
const step = ref<'connection' | 'directory'>(
  search.get('step') === 'directory' ? 'directory' : 'connection',
);

const open = ref(true);
const connecting = search.get('connecting') === 'true';
const connectionError = search.has('error')
  ? 'The remote connection failed. Check the connection and try again.'
  : '';
const connectionString = ref('ssh user@example -p 1234');
const directoryFilter = ref('');
const selectedIndex = ref(0);

function showDirectories(): void {
  step.value = 'directory';
}

const directoryOptions = Array.from({ length: 24 }, (_, index) => ({
  name: `project-${index}`,
  path: `/home/user/project-${index}`,
  kind: 'forward' as const,
}));
</script>

<style scoped>
.fixture-shell {
  width: 100%;
  height: 100%;
  background: var(--canvas);
}
</style>
