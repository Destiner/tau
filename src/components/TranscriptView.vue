<script setup lang="ts">
import { useVirtualizer } from "@tanstack/vue-virtual";
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onMounted, ref, watch } from "vue";
import type { TranscriptEntry } from "../types";
import MarkdownText from "./MarkdownText.vue";
import PiSpinner from "./PiSpinner.vue";
import UiIcon from "./UiIcon.vue";

const props = defineProps<{
  messages: TranscriptEntry[];
  showWorkingIndicator: boolean;
  workingLabel: string;
}>();

const transcript = ref<HTMLElement>();
const workingRowKey = "tau-working-indicator";

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

onMounted(() => {
  void nextTick(scrollToEnd);
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
              <div
                v-else
                class="tool-row"
                :class="{ error: messageAt(virtualRow.index)?.toolErrored }"
              >
                <span class="tool-copy">
                  <span class="tool-name">{{
                    messageAt(virtualRow.index)?.toolName || "tool"
                  }}</span>
                  <span
                    v-if="messageAt(virtualRow.index)?.text"
                    class="tool-argument"
                    >{{ messageAt(virtualRow.index)?.text }}</span
                  >
                </span>
                <span
                  v-if="messageAt(virtualRow.index)?.toolRunning"
                  class="tool-running-indicator"
                  role="status"
                  aria-label="Running"
                ></span>
                <UiIcon
                  v-else-if="messageAt(virtualRow.index)?.toolErrored"
                  class="tool-error-icon"
                  name="cross"
                  aria-label="Failed"
                />
              </div>
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
