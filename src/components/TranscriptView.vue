<script setup lang="ts">
import { useVirtualizer } from "@tanstack/vue-virtual";
import type { ComponentPublicInstance } from "vue";
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import type { TranscriptEntry } from "../types";
import ErrorNotice from "./ErrorNotice.vue";
import MarkdownText from "./MarkdownText.vue";
import PiSpinner from "./PiSpinner.vue";
import ToolCall from "./ToolCall.vue";

const props = defineProps<{
  messages: TranscriptEntry[];
  showWorkingIndicator: boolean;
  workingLabel: string;
}>();

const transcript = ref<HTMLElement>();
const workingRowKey = "tau-working-indicator";

/**
 * Which tool rows are open, held by message id rather than in the row itself:
 * the virtualizer unmounts a row the reader scrolls away from, and an opened
 * call should still be open when they scroll back to it.
 */
const expandedTools = ref(new Set<string>());

/** How far from the end the reader may sit and still be counted as following. */
const followThreshold = 48;

/**
 * Whether output should follow the end. The virtualizer only follows appends,
 * which misses most of a turn: rows grow while they stream, and the working
 * indicator is swapped for the row it was standing in for at an unchanged row
 * count. Tracking the reader's own position instead follows every change, and
 * leaves history alone the moment they scroll away from the end.
 */
let following = true;

const rowVirtualizer = useVirtualizer(
  computed(() => {
    const messages = props.messages;
    const showWorkingIndicator = props.showWorkingIndicator;

    return {
      count: messages.length + (showWorkingIndicator ? 1 : 0),
      getScrollElement: () => transcript.value ?? null,
      estimateSize: (index: number) => estimateRowSize(messages[index]),
      getItemKey: (index: number) => messages[index]?.id ?? workingRowKey,
      anchorTo: "end" as const,
      scrollEndThreshold: followThreshold,
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
    props.messages[props.messages.length - 1]?.id ?? "",
    props.showWorkingIndicator,
    totalSize.value,
  ].join("|"),
);

let viewportObserver: ResizeObserver | undefined;

onMounted(() => {
  void nextTick(scrollToEnd);

  const element = transcript.value;
  if (!element || typeof ResizeObserver === "undefined") return;
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

onBeforeUnmount(() => {
  viewportObserver?.disconnect();
});

watch(contentSignature, () => {
  if (!following) return;
  void nextTick(() => {
    if (following) rowVirtualizer.value.scrollToEnd();
  });
});

function scrollToEnd() {
  following = true;
  rowVirtualizer.value.scrollToEnd();
}

function handleScroll() {
  const element = transcript.value;
  if (!element) return;
  following =
    element.scrollHeight - element.scrollTop - element.clientHeight <=
    followThreshold;
}

function measureRow(element: Element | ComponentPublicInstance | null) {
  if (element instanceof HTMLElement) {
    rowVirtualizer.value.measureElement(element);
  }
}

function messageAt(index: number): TranscriptEntry | undefined {
  return props.messages[index];
}

function isToolExpanded(index: number): boolean {
  const id = messageAt(index)?.id;
  return id ? expandedTools.value.has(id) : false;
}

function toggleTool(index: number) {
  const id = messageAt(index)?.id;
  if (!id) return;
  const next = new Set(expandedTools.value);
  if (!next.delete(id)) next.add(id);
  expandedTools.value = next;
}

function isCompact(index: number): boolean {
  const current = messageAt(index)?.kind;
  const next = messageAt(index + 1)?.kind;
  return isActivity(current) && isActivity(next);
}

function isActivity(
  kind: TranscriptEntry["kind"] | undefined,
): kind is "thinking" | "tool" {
  return kind === "thinking" || kind === "tool";
}

function estimateRowSize(message: TranscriptEntry | undefined): number {
  if (!message) return 42;
  if (message.kind === "tool") return 58;
  if (message.kind === "error") return 88;
  if (message.kind === "user") return 76;
  if (message.kind === "thinking") return 112;
  return 144;
}

defineExpose({ scrollToEnd });
</script>

<template>
  <section
    ref="transcript"
    class="transcript"
    aria-label="Tau transcript"
    @scroll="handleScroll"
  >
    <div v-if="virtualRows.length" class="message-list-shell">
      <div class="message-list" :style="{ height: `${totalSize}px` }">
        <div
          class="message-window"
          :style="{ transform: `translateY(${firstRowOffset}px)` }"
        >
          <template v-for="virtualRow in virtualRows" :key="virtualRow.key">
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
              />
              <MarkdownText
                v-else-if="messageAt(virtualRow.index)?.kind === 'assistant'"
                :source="messageAt(virtualRow.index)?.text ?? ''"
              />
              <div
                v-else-if="messageAt(virtualRow.index)?.kind === 'thinking'"
                class="thinking-block"
              >
                <div class="thinking-label">Thinking</div>
                <MarkdownText
                  :source="messageAt(virtualRow.index)?.text ?? ''"
                />
              </div>
              <ErrorNotice
                v-else-if="messageAt(virtualRow.index)?.kind === 'error'"
                :text="messageAt(virtualRow.index)?.text ?? ''"
              />
              <ToolCall
                v-else
                :entry="messageAt(virtualRow.index)!"
                :expanded="isToolExpanded(virtualRow.index)"
                @toggle="toggleTool(virtualRow.index)"
              />
            </article>

            <div
              v-else
              :ref="measureRow"
              :data-index="virtualRow.index"
              class="stream-state transcript-stream-state"
            >
              <PiSpinner :label="workingLabel" />
            </div>
          </template>
        </div>
      </div>
    </div>
  </section>
</template>
