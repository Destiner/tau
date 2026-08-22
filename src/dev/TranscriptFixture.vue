<template>
  <main class="transcript-fixture">
    <header class="transcript-fixture-header">
      <strong>Long transcript fixture</strong>
      <span data-testid="fixture-session">{{ sessionKey }}</span>
      <span data-testid="fixture-count">{{ messages.length }} messages</span>
    </header>
    <TranscriptView
      :key="sessionKey"
      ref="transcriptView"
      class="fixture-transcript"
      :messages="messages"
      :show-working-indicator="showWorkingIndicator"
      working-label="Pi is working"
      :session-key="sessionKey"
    />
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';

import TranscriptView from '../components/TranscriptView.vue';

import createLongTranscript from './long-transcript';

interface TranscriptFixtureApi {
  appendMessage(kind?: 'assistant' | 'tool'): string;
  truncate(count: number): void;
  hydrate(count: number): void;
  replaceLatestMessage(): string;
  scrollToEnd(): void;
  setWorking(working: boolean): void;
  streamLatest(chunks?: number): Promise<void>;
  switchSession(key: string): void;
}

declare global {
  interface Window {
    __TAU_TRANSCRIPT_FIXTURE__?: TranscriptFixtureApi;
  }
}

/**
 * How many messages to start with. A short transcript is a case of its own:
 * one that does not fill the viewport cannot scroll, so anything the
 * virtualizer asks the element to do is silently clamped.
 */
const initialCount = Number(
  new URLSearchParams(window.location.search).get('count') ?? '',
);
const messages = ref(
  createLongTranscript(
    Number.isFinite(initialCount) && initialCount >= 0
      ? initialCount
      : undefined,
  ),
);
const working = ref(false);
/** Mirrors the app: a session change arrives as a keyed remount. */
const sessionKey = ref('main');
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

/** Mirrors a session opened mid-turn: history lands under the indicator. */
function hydrate(count: number): void {
  messages.value = createLongTranscript(count);
  sequence = count;
}

/** Mirrors compaction: the transcript is replaced by a much shorter one. */
function truncate(count: number): void {
  messages.value = messages.value.slice(-count).map((message, index) => ({
    ...message,
    id: `${message.kind}-${index}`,
  }));
}

function scrollToEnd(): void {
  transcriptView.value?.scrollToEnd();
}

function setWorking(next: boolean): void {
  working.value = next;
}

function switchSession(key: string): void {
  sessionKey.value = key;
}

window.__TAU_TRANSCRIPT_FIXTURE__ = {
  appendMessage,
  truncate,
  hydrate,
  replaceLatestMessage,
  scrollToEnd,
  setWorking,
  streamLatest,
  switchSession,
};

onBeforeUnmount(() => {
  delete window.__TAU_TRANSCRIPT_FIXTURE__;
});
</script>

<style scoped>
.transcript-fixture {
  display: grid;
  grid-template-rows: 38px minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

.transcript-fixture-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px;
  border-bottom: 1px solid var(--border);
  color: var(--muted);
  font-size: 11px;
}

.transcript-fixture-header strong {
  color: var(--text);
  font-weight: 500;
}

.fixture-transcript {
  min-height: 0;
}
</style>
