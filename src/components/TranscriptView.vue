<template>
  <section
    ref="transcript"
    class="transcript"
    aria-label="Tau transcript"
    @scroll="handleScroll"
  >
    <div
      v-if="virtualRows.length || prompt"
      class="message-list-shell"
    >
      <div
        class="message-list"
        :style="{ height: `${totalSize}px` }"
      >
        <div
          class="message-window"
          :style="{ transform: `translateY(${firstRowOffset}px)` }"
        >
          <template
            v-for="virtualRow in virtualRows"
            :key="virtualRow.key"
          >
            <article
              v-if="messageAt(virtualRow.index)"
              :ref="measureRow"
              :data-index="virtualRow.index"
              :data-message-id="messageAt(virtualRow.index)?.id"
              class="message"
              :class="[
                messageAt(virtualRow.index)?.kind,
                { compact: isCompact(virtualRow.index) },
              ]"
            >
              <MarkdownText
                v-if="messageAt(virtualRow.index)?.kind === 'user'"
                class="user-bubble"
                :source="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="basePath"
              />
              <MarkdownText
                v-else-if="messageAt(virtualRow.index)?.kind === 'assistant'"
                :source="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="basePath"
              />
              <ErrorNotice
                v-else-if="messageAt(virtualRow.index)?.kind === 'error'"
                :text="messageAt(virtualRow.index)?.text ?? ''"
              />
              <TranscriptNotice
                v-else-if="messageAt(virtualRow.index)?.kind === 'notice'"
                :type="messageAt(virtualRow.index)?.noticeType ?? 'info'"
                :text="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="messageAt(virtualRow.index)?.basePath"
              />
              <ActivityRow
                v-else
                :entry="messageAt(virtualRow.index)!"
                :expanded="isEntryExpanded(virtualRow.index)"
                :base-path="basePath"
                @toggle="() => toggleEntry(virtualRow.index)"
              />
            </article>

            <div
              v-else
              :ref="measureRow"
              :data-index="virtualRow.index"
              class="stream-state transcript-stream-state"
            >
              <UiSpinner :label="workingLabel" />
            </div>
          </template>
        </div>
      </div>

      <!--
        An interactive prompt is the last thing in the transcript rather than a
        row of it: the virtualizer unmounts rows the reader scrolls away from,
        which would drop the focus and the caret of a prompt still waiting to
        be answered.
      -->
      <div
        v-if="prompt"
        ref="promptRow"
        class="message prompt"
      >
        <ExtensionDialog
          :draft="prompt.draft"
          :method="prompt.method"
          :title="prompt.title"
          :message="prompt.message"
          :options="prompt.options"
          :placeholder="prompt.placeholder"
          :working-directory="prompt.workingDirectory"
          @update:draft="forwardPromptDraft"
          @submit="forwardPromptSubmit"
          @cancel="forwardPromptCancel"
        />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { useVirtualizer } from '@tanstack/vue-virtual';
import type { ComponentPublicInstance } from 'vue';
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import type { ExtensionDialog as ExtensionPrompt } from '../composables/state';
import type { TranscriptEntry } from '../lib/pi/transcript';
import { recallScroll, rememberScroll } from '../lib/transcript-scroll';

import ActivityRow from './ActivityRow.vue';
import ErrorNotice from './ErrorNotice.vue';
import ExtensionDialog from './ExtensionDialog.vue';
import TranscriptNotice from './TranscriptNotice.vue';
import MarkdownText from './ui/MarkdownText.vue';
import UiSpinner from './ui/UiSpinner.vue';

const props = defineProps<{
  messages: TranscriptEntry[];
  showWorkingIndicator: boolean;
  workingLabel: string;
  basePath?: string;
  /** Session this transcript belongs to, under which its position is kept. */
  sessionKey?: string;
  /** The interactive prompt this session is waiting on, if there is one. */
  prompt?: ExtensionPrompt;
}>();

const emit = defineEmits<{
  'prompt-submit': [value: string | boolean];
  'prompt-cancel': [];
  'prompt-draft': [value: string];
}>();

const transcript = ref<HTMLElement>();
const promptRow = ref<HTMLElement>();
const workingRowKey = 'tau-working-indicator';

/**
 * Which activity rows are open, held by message id rather than in the row
 * itself: the virtualizer unmounts rows the reader scrolls away from.
 */
const expandedEntries = ref(new Set<string>());

/** How far from the end the reader may sit and still be counted as following. */
const followThreshold = 48;

/**
 * Where this session was last left, if it has been read before. Taken once:
 * the transcript is keyed by session, so another session arrives as another
 * component rather than as a change to this one.
 */
const restored = recallScroll(props.sessionKey ?? '');

/**
 * Whether output should follow the end. The virtualizer only follows appends,
 * which misses most of a turn: rows grow while they stream, and the working
 * indicator is swapped for the row it was standing in for at an unchanged row
 * count. Tracking the reader's own position instead follows every change, and
 * leaves history alone the moment they scroll away from the end.
 */
let following = restored?.following ?? true;

/**
 * The offset the last scroll was seen at, which is how a scroll the reader
 * made is told apart from one the content caused.
 */
let lastScrollOffset = restored?.offset ?? 0;

const rowVirtualizer = useVirtualizer(
  computed(() => {
    const messages = props.messages;
    const showWorkingIndicator = props.showWorkingIndicator;

    return {
      count: messages.length + (showWorkingIndicator ? 1 : 0),
      getScrollElement: (): HTMLElement | null => transcript.value ?? null,
      estimateSize: (index: number): number => estimateRowSize(messages[index]),
      getItemKey: (index: number): string =>
        messages[index]?.id ?? workingRowKey,
      anchorTo: 'end' as const,
      scrollEndThreshold: followThreshold,
      /**
       * The measured sizes this session was left with, so the first render is
       * laid out as the reader left it rather than out of estimates, and the
       * offset restored below means the same content it did then.
       */
      initialMeasurementsCache: restored?.measurements,
      initialOffset: restored?.offset,
      overscan: 8,
      paddingStart: 28,
      paddingEnd: 30,
    };
  }),
);

const virtualRows = computed(() => rowVirtualizer.value.getVirtualItems());
const totalSize = computed(() => rowVirtualizer.value.getTotalSize());
const firstRowOffset = computed(() => virtualRows.value[0]?.start ?? 0);

/**
 * Changes worth following, as one comparable value: rows appended or dropped,
 * the indicator appearing, and — through the measured total — a row growing as
 * it streams. Scrolling alone leaves it untouched.
 */
const contentSignature = computed(() =>
  [
    props.messages.length,
    props.messages[props.messages.length - 1]?.id ?? '',
    props.showWorkingIndicator,
    totalSize.value,
    props.prompt?.key ?? '',
  ].join('|'),
);

let viewportObserver: ResizeObserver | undefined;

onMounted(() => {
  void nextTick(restoreScroll);

  const element = transcript.value;
  if (!element || typeof ResizeObserver === 'undefined') return;
  /**
   * The end also moves when the viewport shrinks under a reader sitting at it:
   * the composer takes a line as they type, a status line arrives, the window or
   * the sidebar is resized. Scroll position survives all of those, so the end
   * slides down by whatever height the transcript gave up. Being resized is not
   * the reader scrolling away, so following is left as it was and the end is
   * taken up again.
   *
   * The end is asked for as an offset past it rather than through scrollToEnd,
   * which measures against the viewport size the virtualizer has cached: its own
   * observer may not have run yet, and aiming at the height the transcript has
   * just given up lands exactly that far short. An offset is clamped to the live
   * maximum instead, and unlike a bare scrollTop write it is one the virtualizer
   * knows about, so its next update does not undo it.
   */
  viewportObserver = new ResizeObserver(() => {
    if (following) rowVirtualizer.value.scrollToOffset(element.scrollHeight);
  });
  viewportObserver.observe(element);
});

/**
 * The prompt sits below the rows the virtualizer measures, so its height is
 * part of the end without being part of the total it knows about. The same
 * clamped offset the viewport uses takes that end up as the prompt grows.
 */
watch(promptRow, (row, previous) => {
  if (previous) viewportObserver?.unobserve(previous);
  if (row) viewportObserver?.observe(row);
});

/**
 * A prompt is a question the session cannot go on without, so it is brought to
 * the reader wherever they were — the composer it replaces was never a place
 * they could scroll away from.
 */
watch(
  () => props.prompt?.key,
  (key) => {
    if (key) void nextTick(scrollToEnd);
  },
);

onBeforeUnmount(() => {
  viewportObserver?.disconnect();
  rememberScroll(props.sessionKey ?? '', {
    offset: rowVirtualizer.value.scrollOffset ?? 0,
    following,
    measurements: rowVirtualizer.value.takeSnapshot(),
  });
});

watch(contentSignature, () => {
  if (!following) return;
  void nextTick(() => {
    if (following) scrollToLatest();
  });
});

/**
 * Takes up the position this session was left at. A reader who left at the end
 * is given the end as it stands now rather than the pixel it was then, since
 * the session goes on streaming while it is off screen.
 *
 * The offset is asked for rather than written to scrollTop directly: the
 * virtualizer starts out believing it, and only a scroll it made itself keeps
 * its next update from undoing it.
 */
function restoreScroll(): void {
  if (following) {
    scrollToEnd();
    return;
  }
  rowVirtualizer.value.scrollToOffset(restored?.offset ?? 0);
}

function scrollToEnd(): void {
  following = true;
  scrollToLatest();
}

/**
 * The end of the transcript, asked for as an offset past it: it is clamped to
 * the live maximum, and unlike a bare scrollTop write it is one the
 * virtualizer knows about.
 *
 * The last row is not the end. The bottom padding, and a prompt when the
 * session is waiting on an answer, sit below it, and aiming at the row leaves
 * the reader short of both.
 */
function scrollToLatest(): void {
  const element = transcript.value;
  if (element) {
    rowVirtualizer.value.scrollToOffset(element.scrollHeight);
    return;
  }
  rowVirtualizer.value.scrollToEnd();
}

/**
 * Leaving the end is something the reader does, not something that happens to
 * them: a row that grows after it was measured — an image that has only just
 * decoded — moves the end away from a reader who has not scrolled at all, and
 * reading that as leaving would strand them a row short of everything that
 * followed. Only a scroll back up gives up the end; scrolling down to it takes
 * it up again.
 */
function handleScroll(): void {
  const element = transcript.value;
  if (!element) return;

  const offset = element.scrollTop;
  const atEnd =
    element.scrollHeight - offset - element.clientHeight <= followThreshold;
  following = offset < lastScrollOffset ? atEnd : following || atEnd;
  lastScrollOffset = offset;
}

function forwardPromptDraft(value: string): void {
  emit('prompt-draft', value);
}

function forwardPromptSubmit(value: string | boolean): void {
  emit('prompt-submit', value);
}

function forwardPromptCancel(): void {
  emit('prompt-cancel');
}

function measureRow(element: Element | ComponentPublicInstance | null): void {
  if (element instanceof HTMLElement) {
    rowVirtualizer.value.measureElement(element);
  }
}

function messageAt(index: number): TranscriptEntry | undefined {
  return props.messages[index];
}

function isEntryExpanded(index: number): boolean {
  const id = messageAt(index)?.id;
  return id ? expandedEntries.value.has(id) : false;
}

function toggleEntry(index: number): void {
  const id = messageAt(index)?.id;
  if (!id) return;
  const next = new Set(expandedEntries.value);
  if (!next.delete(id)) next.add(id);
  expandedEntries.value = next;
}

function isCompact(index: number): boolean {
  const current = messageAt(index)?.kind;
  const next = messageAt(index + 1)?.kind;
  return isActivity(current) && isActivity(next);
}

function isActivity(
  kind: TranscriptEntry['kind'] | undefined,
): kind is 'thinking' | 'tool' | 'skill' {
  return kind === 'thinking' || kind === 'tool' || kind === 'skill';
}

function estimateRowSize(message: TranscriptEntry | undefined): number {
  if (!message) return 42;
  if (isActivity(message.kind)) return 26;
  if (message.kind === 'error' || message.kind === 'notice') return 88;
  if (message.kind === 'user') return 76;
  return 144;
}

defineExpose({ scrollToEnd });
</script>

<style scoped>
.transcript {
  min-height: 0;
  overflow: hidden auto;
  overflow-anchor: none;
  overscroll-behavior: contain;
}

.message-list-shell {
  width: min(100%, 880px);
  margin: 0 auto;
  padding: 0 clamp(20px, 5vw, 60px);
}

.message-list {
  position: relative;
  width: 100%;
}

.message-window {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
}

.message {
  margin: 0;
  padding-bottom: 22px;
}

.message.user {
  display: flex;
  justify-content: flex-start;
  margin-right: 20px;
  margin-left: -5px;
}

.user-bubble {
  max-width: min(78%, 650px);
  padding: 9px 13px;
  border-radius: 7px;
  background: var(--user);
}

/*
 * Code and table headers sit on the bubble rather than the canvas, and their
 * shared sunk background is too close to it to read as a surface of their
 * own. The selectors are deep because both live in MarkdownText's rendered
 * HTML.
 */
.user-bubble :deep(code),
.user-bubble :deep(pre),
.user-bubble :deep(th) {
  background: var(--panel-raised);
}

.message.assistant,
.message.error,
.message.notice,
.message.prompt,
.stream-state {
  margin-right: 20px;
  margin-left: 8px;
}

/*
 * Activity is a line of text rather than a block, so it hangs off the same
 * left edge as the reply it belongs to instead of being indented under it.
 */
.message.thinking,
.message.tool,
.message.skill {
  margin-right: 20px;
  margin-left: 8px;
}

/* A run of activity is one thing; only its last row is followed by a gap. */
.message.compact {
  padding-bottom: 0;
}

.stream-state {
  display: flex;
  align-items: center;
  color: var(--muted);
}

.transcript-stream-state {
  padding-bottom: 22px;
}
</style>
