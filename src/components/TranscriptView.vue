<template>
  <section
    ref="transcript"
    class="transcript"
    aria-label="Transcript"
    @scroll="handleScroll"
  >
    <div
      v-if="virtualRows.length || compacting || prompt"
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
              :data-pending="
                messageAt(virtualRow.index)?.pending ? 'true' : undefined
              "
              class="message"
              :class="[
                messageAt(virtualRow.index)?.kind,
                {
                  compact: isCompact(virtualRow.index),
                  pending: messageAt(virtualRow.index)?.pending,
                },
              ]"
            >
              <MarkdownText
                v-if="messageAt(virtualRow.index)?.kind === 'user'"
                class="user-bubble"
                :source="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="basePath"
                :copy-paths="copyPaths"
                :remote-project-path="remoteProjectPath"
              />
              <MarkdownText
                v-else-if="messageAt(virtualRow.index)?.kind === 'assistant'"
                :source="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="basePath"
                :copy-paths="copyPaths"
                :remote-project-path="remoteProjectPath"
              />
              <CompactionDivider
                v-else-if="messageAt(virtualRow.index)?.kind === 'compaction'"
                :entry="messageAt(virtualRow.index)!"
                @load="
                  (element) => requestEarlierHistory(virtualRow.index, element)
                "
              />
              <ErrorNotice
                v-else-if="messageAt(virtualRow.index)?.kind === 'error'"
                :text="messageAt(virtualRow.index)?.text ?? ''"
                :label="messageAt(virtualRow.index)?.errorLabel"
              />
              <TranscriptNotice
                v-else-if="messageAt(virtualRow.index)?.kind === 'notice'"
                :type="messageAt(virtualRow.index)?.noticeType ?? 'info'"
                :text="messageAt(virtualRow.index)?.text ?? ''"
                :base-path="messageAt(virtualRow.index)?.basePath"
                :copy-paths="copyPaths"
                :remote-project-path="remoteProjectPath"
              />
              <ActivityRow
                v-else
                :entry="messageAt(virtualRow.index)!"
                :expanded="isEntryExpanded(virtualRow.index)"
                :base-path="basePath"
                :copy-paths="copyPaths"
                :remote-project-path="remoteProjectPath"
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

      <div
        v-if="compacting"
        ref="compactionRow"
        class="message compaction transient-compaction"
        role="status"
      >
        <CompactionDivider
          :entry="compactingEntry"
          label="compacting"
        />
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
          :key="prompt.key"
          :draft="prompt.draft"
          :method="prompt.method"
          :title="prompt.title"
          :message="prompt.message"
          :options="prompt.options"
          :placeholder="prompt.placeholder"
          :submitting="prompt.submitting"
          :disabled="promptDisabled"
          :error="prompt.error"
          :working-directory="prompt.workingDirectory"
          :copy-paths="copyPaths"
          :remote-project-path="remoteProjectPath"
          @update:draft="forwardPromptDraft"
          @submit="forwardPromptSubmit"
          @cancel="forwardPromptCancel"
        />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import {
  measureElement as measureVirtualElement,
  useVirtualizer,
} from '@tanstack/vue-virtual';
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
import CompactionDivider from './CompactionDivider.vue';
import ErrorNotice from './ErrorNotice.vue';
import ExtensionDialog from './ExtensionDialog.vue';
import TranscriptNotice from './TranscriptNotice.vue';
import MarkdownText from './ui/MarkdownText.vue';
import UiSpinner from './ui/UiSpinner.vue';

const props = defineProps<{
  messages: TranscriptEntry[];
  compacting: boolean;
  showWorkingIndicator: boolean;
  workingLabel: string;
  basePath?: string;

  copyPaths?: boolean;
  remoteProjectPath?: string;

  sessionKey?: string;

  prompt?: ExtensionPrompt;
  promptDisabled?: boolean;
}>();

const emit = defineEmits<{
  'prompt-submit': [value: string | boolean];
  'prompt-cancel': [];
  'prompt-draft': [value: string];
  'load-history': [];
}>();

const transcript = ref<HTMLElement>();
const promptRow = ref<HTMLElement>();
const compactionRow = ref<HTMLElement>();
const workingRowKey = 'tau-working-indicator';
const compactingEntry: TranscriptEntry = {
  id: 'tau-compacting',
  kind: 'compaction',
  text: '',
};

const expandedEntries = ref(new Set<string>());

const followThreshold = 48;

const restored = recallScroll(props.sessionKey ?? '');

/**
 * Whether output should follow the end. The virtualizer only follows appends,
 * which misses most of a turn: rows grow while they stream, and the working
 * indicator is swapped for the row it was standing in for at an unchanged row
 * count. Tracking the reader's own position instead follows every change, and
 * leaves history alone the moment they scroll away from the end.
 */
let following = restored?.following ?? true;

let lastScrollOffset = restored?.offset ?? 0;

const rowVirtualizer = useVirtualizer<HTMLElement, HTMLElement>(
  computed(() => {
    const messages = props.messages;
    const showWorkingIndicator = props.showWorkingIndicator;

    return {
      count: messages.length + (showWorkingIndicator ? 1 : 0),
      getScrollElement: (): HTMLElement | null => transcript.value ?? null,
      estimateSize: (index: number): number => estimateRowSize(messages[index]),
      getItemKey: (index: number): string =>
        messages[index]?.id ?? workingRowKey,
      /**
       * Vue calls row refs while WebKit is still attaching their descendants.
       * An offsetHeight read there can cache that incomplete height until the
       * ResizeObserver runs, briefly drawing the end window below the fold.
       * While following the end, keep the estimate for a row's first
       * synchronous pass; observer entries and already-settled measurements
       * remain authoritative. A reader in history still needs synchronous
       * measurements so prepended rows can restore their anchor immediately.
       */
      measureElement: (element, entry, instance): number => {
        if (entry || !following)
          return measureVirtualElement(element, entry, instance);
        const index = instance.indexFromElement(element);
        const key = instance.options.getItemKey(index);
        return (
          instance.itemSizeCache.get(key) ??
          instance.options.estimateSize(index)
        );
      },
      anchorTo: 'end' as const,
      scrollEndThreshold: followThreshold,

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

const contentSignature = computed(() =>
  [
    props.messages.length,
    props.messages[props.messages.length - 1]?.id ?? '',
    props.showWorkingIndicator,
    props.compacting,
    totalSize.value,
    props.prompt?.key ?? '',
  ].join('|'),
);
const historySignature = computed(() =>
  [props.messages.length, props.messages[0]?.id ?? ''].join('|'),
);

let pendingHistoryAnchor:
  { messageId: string; viewportTop: number } | undefined;

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
    reconcileUnscrollableOffset();
  });
  viewportObserver.observe(element);
});

/**
 * The prompt sits below the rows the virtualizer measures, so its height is
 * part of the end without being part of the total it knows about. The same
 * clamped offset the viewport uses takes that end up as the prompt grows.
 */
for (const bottomRow of [compactionRow, promptRow]) {
  watch(bottomRow, (row, previous) => {
    if (previous) viewportObserver?.unobserve(previous);
    if (row) viewportObserver?.observe(row);
  });
}

onBeforeUnmount(() => {
  viewportObserver?.disconnect();
  const element = transcript.value;
  const offset =
    element && element.scrollHeight <= element.clientHeight
      ? element.scrollTop
      : (rowVirtualizer.value.scrollOffset ?? 0);
  rememberScroll(props.sessionKey ?? '', {
    offset,
    following,
    measurements: rowVirtualizer.value.takeSnapshot(),
  });
});

watch(contentSignature, () => {
  void nextTick(() => {
    if (following) scrollToLatest();
    else reconcileUnscrollableOffset();
  });
});

watch(historySignature, () => {
  if (pendingHistoryAnchor) void nextTick(restoreHistoryAnchor);
});

function restoreScroll(): void {
  if (following) {
    scrollToEnd();
    return;
  }
  rowVirtualizer.value.scrollToOffset(restored?.offset ?? 0);
  reconcileUnscrollableOffset();
}

function scrollToEnd(): void {
  following = true;
  scrollToLatest();
}

function scrollToLatest(): void {
  const element = transcript.value;
  if (element) {
    rowVirtualizer.value.scrollToOffset(element.scrollHeight);
    reconcileUnscrollableOffset();
    return;
  }
  rowVirtualizer.value.scrollToEnd();
}

function reconcileUnscrollableOffset(): void {
  const element = transcript.value;
  if (
    element &&
    element.scrollHeight <= element.clientHeight &&
    element.scrollTop === 0 &&
    (rowVirtualizer.value.scrollOffset ?? 0) !== 0
  ) {
    element.dispatchEvent(new Event('scroll'));
  }
}

function handleScroll(): void {
  const element = transcript.value;
  if (!element) return;

  const offset = element.scrollTop;
  const atEnd =
    element.scrollHeight - offset - element.clientHeight <= followThreshold;
  following = offset < lastScrollOffset ? atEnd : following || atEnd;
  lastScrollOffset = offset;
}

function requestEarlierHistory(index: number, element: HTMLElement): void {
  const messageId = messageAt(index)?.id;
  if (!messageId) return;
  /*
   * Measured from the row rather than the divider inside it, because that is
   * the box the anchor is restored against. The row contains the divider's
   * vertical margins, so the two do not share a top edge, and mixing them
   * moves the divider by one margin on every load.
   */
  const row = element.closest<HTMLElement>('[data-message-id]') ?? element;
  pendingHistoryAnchor = {
    messageId,
    viewportTop: row.getBoundingClientRect().top,
  };
  following = false;
  emit('load-history');
}

async function restoreHistoryAnchor(): Promise<void> {
  const anchor = pendingHistoryAnchor;
  if (!anchor) return;
  const index = props.messages.findIndex(
    (message) => message.id === anchor.messageId,
  );
  if (index < 0) {
    pendingHistoryAnchor = undefined;
    return;
  }

  rowVirtualizer.value.scrollToIndex(index, { align: 'start' });
  await nextTick();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const row = [
    ...(transcript.value?.querySelectorAll<HTMLElement>('[data-message-id]') ??
      []),
  ].find((element) => element.dataset.messageId === anchor.messageId);
  if (row) {
    const offset =
      rowVirtualizer.value.scrollOffset ?? transcript.value?.scrollTop ?? 0;
    rowVirtualizer.value.scrollToOffset(
      offset + row.getBoundingClientRect().top - anchor.viewportTop,
    );
  }
  pendingHistoryAnchor = undefined;
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
  if (message.kind === 'compaction') return 54;
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

.message.user.pending {
  opacity: 0.88;
}

.message.user :deep(.user-bubble) {
  width: fit-content;
  max-width: 90%;
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
.message.user :deep(.user-bubble code),
.message.user :deep(.user-bubble pre),
.message.user :deep(.user-bubble th) {
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

/* Contain the divider's vertical margins so its measured row includes the full
 * 54px rhythm instead of reporting only the 22px control. */
.message.compaction {
  display: flow-root;
  margin-right: 20px;
  margin-left: 8px;
  padding-bottom: 0;
}

.transient-compaction {
  margin-top: -30px;
}

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
