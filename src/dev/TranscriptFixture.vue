<template>
  <main class="transcript-fixture">
    <header class="transcript-fixture-header">
      <strong>Long transcript fixture</strong>
      <span data-testid="fixture-session">{{ sessionKey }}</span>
      <span data-testid="fixture-count">{{ messages.length }} messages</span>
    </header>
    <TranscriptSearch
      v-if="search && find.current.value.open"
      :query="find.current.value.query"
      :total="find.current.value.matches.length"
      :current="findOrdinal"
      :busy="find.current.value.busy"
      :pending="find.current.value.pending"
      @query="find.setQuery"
      @navigate="find.navigate"
      @close="find.close"
    />
    <TranscriptView
      :key="sessionKey"
      ref="transcriptView"
      class="fixture-transcript"
      :messages="messages"
      :compacting="false"
      :show-working-indicator="showWorkingIndicator"
      working-label="Working"
      :base-path="remote ? '/home/agent/rhinestone' : '/Users/someone/code/tau'"
      :copy-paths="remote"
      :remote-project-path="remote ? 'ssh:fixture-project' : undefined"
      :session-key="sessionKey"
      :search-open="search && find.current.value.open"
      :search-matches="find.current.value.matches"
      :active-search-match="find.current.value.activeMatch ?? undefined"
      @load-history="loadCompactedHistory"
    />
  </main>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import TranscriptSearch from '../components/TranscriptSearch.vue';
import TranscriptView from '../components/TranscriptView.vue';
import { useTranscriptFind } from '../composables/useTranscriptFind';

import createLongTranscript, {
  createCompactToolTranscript,
  createPreviewTranscript,
  createShowcaseTranscript,
  createVerboseToolTranscript,
} from './long-transcript';
import createGeometryTranscript from './transcript-geometry';

interface TranscriptFixtureApi {
  appendMessage(kind?: 'assistant' | 'tool'): string;
  truncate(count: number): void;
  hydrate(count: number): void;
  replaceLatestMessage(): string;
  updateMessage(id: string, text: string): void;
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
const parameters = new URLSearchParams(window.location.search);
const requestedCount = parameters.get('count');
const search = parameters.get('search') === 'true';
const compactTools = parameters.get('compact-tools') === 'true';
const verboseTool = parameters.get('verbose-tool') === 'true';
const showcase = parameters.get('showcase') === 'true';
const preview = parameters.get('preview') === 'true';
const remote = parameters.get('remote') === 'true';
const compacted = parameters.get('compacted') === 'true';
const initialCount = Number(requestedCount);
const initialMessages =
  parameters.get('geometry') === 'true'
    ? createGeometryTranscript()
    : verboseTool
      ? createVerboseToolTranscript()
      : compactTools
        ? createCompactToolTranscript()
        : preview
          ? createPreviewTranscript()
          : showcase
            ? createShowcaseTranscript()
            : createLongTranscript(
                requestedCount &&
                  Number.isFinite(initialCount) &&
                  initialCount >= 0
                  ? initialCount
                  : compacted
                    ? 12
                    : undefined,
              );
if (search && initialMessages.length >= 6) {
  for (const [index, label] of [
    [0, 'early'],
    [Math.floor(initialMessages.length / 2), 'middle'],
    [initialMessages.length - 1, 'late'],
  ] as const) {
    initialMessages[index] = {
      id: `search-${label}`,
      kind: index === 0 ? 'user' : 'assistant',
      text: `Search checkpoint ${label}.\n\nCross **format** needle.\n\n\`\`\`ts\nconst codeNeedle = "checkpoint";\nconst wideLine = "${'horizontal padding '.repeat(40)}horizontal-search-target";\n\`\`\`${index === 0 ? `\n\n| ${Array.from({ length: 32 }, (_, column) => `Column ${column}`).join(' | ')} |\n| ${Array(32).fill('---').join(' | ')} |\n| ${Array.from({ length: 32 }, (_, column) => (column === 31 ? 'table-search-target' : 'wide-table-padding')).join(' | ')} |` : ''}`,
    };
  }
  initialMessages[1] = {
    id: 'search-thinking',
    kind: 'thinking',
    text: 'excluded-thinking-needle',
  };
  initialMessages[2] = {
    id: 'search-tool',
    kind: 'tool',
    text: 'excluded-tool-needle',
    toolName: 'read',
    toolCallId: 'search-tool-call',
    toolArguments: '{"path":"excluded-argument-needle"}',
    toolResult: 'excluded-result-needle',
    toolRunning: false,
    toolErrored: false,
  };
  initialMessages[3] = {
    id: 'search-notice',
    kind: 'notice',
    text: 'excluded-notice-needle',
  };
}
const messages = ref(
  compacted
    ? [
        {
          id: 'fixture-compaction',
          kind: 'compaction' as const,
          text: '',
          historyAvailable: true,
          historyLoading: false,
        },
        ...initialMessages.map((message) => ({
          ...message,
          id: `current-${message.id}`,
        })),
      ]
    : initialMessages,
);

if (remote) {
  messages.value.push({
    id: 'fixture-remote-paths',
    kind: 'assistant',
    text: `Workspace \`/home/agent/rhinestone/workspace\`

Source \`src/remote.ts\`

Line reference: src/ranged.ts:42-50

[Encoded file](docs/My%20File.md) · [Local file](file:///tmp/My%20File.txt) · [Invalid file](%zz)

\`\`\`md
Repo /home/agent/rhinestone/orchestrator
Plan /home/agent/.pi/workflows/implement/RHI-6092/implementation-plan.md
/usage
/ expanded
</pre>
\`\`\``,
  });
}
const working = ref(false);

const sessionKey = ref('main');
const transcriptView = ref<InstanceType<typeof TranscriptView>>();
let sequence = messages.value.length;

const controller = computed(() => ({
  key: sessionKey.value,
  sessionId: sessionKey.value,
  sessionOwnerSequence: 0,
  disposed: false,
}));
const find = useTranscriptFind({
  activeController: controller,
  controllers: () => [controller.value],
  messages,
});
const findOrdinal = computed(() => {
  const current = find.current.value;
  return current.activeMatch
    ? current.matches.indexOf(current.activeMatch) + 1
    : 0;
});
watch(find.revealRequest, async (request) => {
  if (!request) return;
  await nextTick();
  await transcriptView.value?.revealSearchMatch(request.match);
});
if (search) find.open();

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

function updateMessage(id: string, text: string): void {
  const message = messages.value.find((entry) => entry.id === id);
  if (message) message.text = text;
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

function loadCompactedHistory(): void {
  const boundary = messages.value.findIndex(
    (message) => message.id === 'fixture-compaction',
  );
  if (boundary < 0 || !messages.value[boundary]?.historyAvailable) return;
  const marker = {
    ...messages.value[boundary]!,
    historyAvailable: false,
    historyLoading: false,
  };
  const older = createLongTranscript(16).map((message, index) => ({
    ...message,
    id: `history-${message.id}`,
    ...(index === 0
      ? { text: 'Search checkpoint from explicitly loaded earlier history.' }
      : {}),
  }));
  messages.value = [
    ...messages.value.slice(0, boundary),
    ...older,
    marker,
    ...messages.value.slice(boundary + 1),
  ];
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
  updateMessage,
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
  position: relative;
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
