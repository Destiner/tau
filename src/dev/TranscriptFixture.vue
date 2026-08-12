<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import TranscriptView from "../components/TranscriptView.vue";
import { createLongTranscript } from "./long-transcript";

interface TranscriptFixtureApi {
  appendMessage(): string;
  replaceLatestMessage(): string;
  scrollToEnd(): void;
  streamLatest(chunks?: number): Promise<void>;
}

declare global {
  interface Window {
    __TAU_TRANSCRIPT_FIXTURE__?: TranscriptFixtureApi;
  }
}

const messages = ref(createLongTranscript());
const streaming = ref(false);
const transcriptView = ref<InstanceType<typeof TranscriptView>>();
let sequence = messages.value.length;

function appendMessage(): string {
  const id = `fixture-appended-${sequence++}`;
  messages.value.push({
    id,
    kind: "assistant",
    text: `**Appended message ${id}**\n\nThe viewport should follow this only when it was already at the end.`,
  });
  return id;
}

function replaceLatestMessage(): string {
  const lastIndex = messages.value.length - 1;
  const last = messages.value[lastIndex];
  if (!last) return "";

  const id = `fixture-rehydrated-${sequence++}`;
  messages.value = messages.value.map((message, index) =>
    index === lastIndex ? { ...message, id } : { ...message },
  );
  return id;
}

async function streamLatest(chunks = 24): Promise<void> {
  const last = messages.value[messages.value.length - 1];
  if (!last || streaming.value) return;

  streaming.value = true;
  for (let index = 0; index < chunks; index += 1) {
    last.text += `\n\nStreaming fixture chunk ${index + 1}. The final row is growing to exercise ResizeObserver anchoring.`;
    await new Promise((resolve) => window.setTimeout(resolve, 16));
  }
  streaming.value = false;
}

function scrollToEnd() {
  transcriptView.value?.scrollToEnd();
}

window.__TAU_TRANSCRIPT_FIXTURE__ = {
  appendMessage,
  replaceLatestMessage,
  scrollToEnd,
  streamLatest,
};

onBeforeUnmount(() => {
  delete window.__TAU_TRANSCRIPT_FIXTURE__;
});
</script>

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
      :show-working-indicator="streaming"
      working-label="Pi is working"
    />
  </main>
</template>

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
