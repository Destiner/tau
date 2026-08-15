<template>
  <main class="transcript-fixture">
    <header class="transcript-fixture-header">
      <strong>Long transcript fixture</strong>
      <span data-testid="fixture-count">{{ messages.length }} messages</span>
    </header>
    <TranscriptView
      ref="transcriptView"
      class="fixture-transcript"
      :messages="messages"
      :show-working-indicator="showWorkingIndicator"
      working-label="Pi is working"
    />
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';

import TranscriptView from '../components/TranscriptView.vue';

import createLongTranscript from './long-transcript';

interface TranscriptFixtureApi {
  appendMessage(kind?: 'assistant' | 'tool'): string;
  replaceLatestMessage(): string;
  scrollToEnd(): void;
  setWorking(working: boolean): void;
  streamLatest(chunks?: number): Promise<void>;
}

declare global {
  interface Window {
    __TAU_TRANSCRIPT_FIXTURE__?: TranscriptFixtureApi;
  }
}

const messages = ref(createLongTranscript());
const working = ref(false);
const transcriptView = ref<InstanceType<typeof TranscriptView>>();
let sequence = messages.value.length;

/** Mirrors the app: the indicator stands in for a reply that has yet to arrive. */
const showWorkingIndicator = computed(
  () =>
    working.value &&
    messages.value[messages.value.length - 1]?.kind !== 'assistant',
);

function appendMessage(kind: 'assistant' | 'tool' = 'assistant'): string {
  const id = `fixture-appended-${sequence++}`;
  if (kind === 'tool') {
    const path = `/tmp/tau-fixture/appended-${id}.jsonl`;
    messages.value.push({
      id,
      kind: 'tool',
      text: path,
      toolCallId: `fixture-appended-call-${id}`,
      toolName: 'read',
      toolRunning: true,
      toolErrored: false,
      toolArguments: JSON.stringify({ path }, null, 2),
    });
    return id;
  }
  messages.value.push({
    id,
    kind: 'assistant',
    text: `**Appended message ${id}**\n\nThe viewport should follow this only when it was already at the end.`,
  });
  return id;
}

function replaceLatestMessage(): string {
  const lastIndex = messages.value.length - 1;
  const last = messages.value[lastIndex];
  if (!last) return '';

  const id = `fixture-rehydrated-${sequence++}`;
  messages.value = messages.value.map((message, index) =>
    index === lastIndex ? { ...message, id } : { ...message },
  );
  return id;
}

async function streamLatest(chunks = 24): Promise<void> {
  const last = messages.value[messages.value.length - 1];
  if (!last || working.value) return;

  working.value = true;
  for (let index = 0; index < chunks; index += 1) {
    last.text += `\n\nStreaming fixture chunk ${index + 1}. The final row is growing to exercise ResizeObserver anchoring.`;
    await new Promise((resolve) => window.setTimeout(resolve, 16));
  }
  working.value = false;
}

function scrollToEnd(): void {
  transcriptView.value?.scrollToEnd();
}

function setWorking(next: boolean): void {
  working.value = next;
}

window.__TAU_TRANSCRIPT_FIXTURE__ = {
  appendMessage,
  replaceLatestMessage,
  scrollToEnd,
  setWorking,
  streamLatest,
};

onBeforeUnmount(() => {
  delete window.__TAU_TRANSCRIPT_FIXTURE__;
});
</script>

<style scoped>
.transcript-fixture {
  display: grid;
  width: 100%;
  height: 100%;
  grid-template-rows: 38px minmax(0, 1fr);
  background: var(--canvas);
}

.transcript-fixture-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border);
  color: var(--muted);
  font-size: 11px;
  padding: 0 12px;
}

.transcript-fixture-header strong {
  color: var(--text);
  font-weight: 500;
}

.fixture-transcript {
  min-height: 0;
}
</style>
