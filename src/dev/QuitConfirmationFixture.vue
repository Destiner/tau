<template>
  <main class="fixture-shell">
    <QuitConfirmation
      :open="open"
      :session-count="sessionCount"
      @cancel="cancel"
      @confirm="confirm"
    />
    <output data-testid="quit-outcome">{{ outcome }}</output>
  </main>
</template>

<script setup lang="ts">
import { ref } from 'vue';

import QuitConfirmation from '../components/QuitConfirmation.vue';

const search = new URLSearchParams(window.location.search);
const requestedCount = Number(search.get('sessions') ?? '2');
const sessionCount = Number.isSafeInteger(requestedCount)
  ? Math.max(1, requestedCount)
  : 2;
const open = ref(true);
const outcome = ref('');

function cancel(): void {
  finish('cancelled');
}

function confirm(): void {
  finish('confirmed');
}

function finish(next: 'cancelled' | 'confirmed'): void {
  outcome.value = next;
  open.value = false;
}
</script>

<style scoped>
.fixture-shell {
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

.fixture-shell output {
  position: absolute;
  top: 8px;
  left: 8px;
  color: var(--muted);
}
</style>
