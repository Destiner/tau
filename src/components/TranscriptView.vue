<script setup lang="ts">
import { useVirtualizer } from "@tanstack/vue-virtual";
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onMounted, ref } from "vue";
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
      followOnAppend: true,
      scrollEndThreshold: 48,
      overscan: 8,
      paddingStart: 28,
      paddingEnd: 30,
    };
  }),
);

const virtualRows = computed(() => rowVirtualizer.value.getVirtualItems());
const totalSize = computed(() => rowVirtualizer.value.getTotalSize());
const firstRowOffset = computed(() => virtualRows.value[0]?.start ?? 0);

onMounted(() => {
  void nextTick(scrollToEnd);
});

function scrollToEnd() {
  rowVirtualizer.value.scrollToEnd();
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
  <section ref="transcript" class="transcript" aria-label="Tau transcript">
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
              <div
                v-if="messageAt(virtualRow.index)?.kind === 'user'"
                class="user-bubble"
              >
                {{ messageAt(virtualRow.index)?.text }}
              </div>
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
