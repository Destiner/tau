<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import MarkdownText from "./components/MarkdownText.vue";
import PiSpinner from "./components/PiSpinner.vue";
import UiIcon from "./components/UiIcon.vue";
import { useTau } from "./composables/useTau";
import {
  latestWindowStart,
  newerWindowStart,
  olderWindowStart,
  transcriptWindowEnd,
} from "./lib/transcript-window";
import type { ThinkingLevel } from "./types";

const transcript = ref<HTMLElement>();
const pinnedToBottom = ref(true);
const transcriptWindowStart = ref(0);
const shiftingWindow = ref(false);
const {
  state,
  activeProject,
  sessionTitle,
  effortLabels,
  settingsDisabled,
  initialize,
  dispose,
  addProject,
  toggleProject,
  removeProject,
  newSession,
  selectSession,
  sendMessage,
  stop,
  selectModel,
  selectEffort,
} = useTau();

const transcriptWindowEndIndex = computed(() =>
  transcriptWindowEnd(state.messages, transcriptWindowStart.value),
);
const visibleMessages = computed(() =>
  state.messages.slice(
    transcriptWindowStart.value,
    transcriptWindowEndIndex.value,
  ),
);

onMounted(() => void initialize());
onBeforeUnmount(dispose);

watch(
  () => state.messages,
  () => {
    transcriptWindowStart.value = latestWindowStart(state.messages);
    pinnedToBottom.value = true;
  },
);

watch(
  () => [
    state.messages.length,
    state.messages[state.messages.length - 1]?.text,
  ],
  () => {
    if (!pinnedToBottom.value) return;
    transcriptWindowStart.value = latestWindowStart(state.messages);
    void nextTick(() =>
      transcript.value?.scrollTo({ top: transcript.value.scrollHeight }),
    );
  },
);

async function handleTranscriptScroll() {
  const element = transcript.value;
  if (!element || shiftingWindow.value) return;
  const nearTop = element.scrollTop < 16;
  const nearBottom =
    element.scrollHeight - element.scrollTop - element.clientHeight < 32;

  if (nearTop && transcriptWindowStart.value > 0) {
    await loadEarlierMessages();
    return;
  }
  if (nearBottom && transcriptWindowEndIndex.value < state.messages.length) {
    await loadNewerMessages();
    return;
  }
  pinnedToBottom.value =
    nearBottom && transcriptWindowEndIndex.value >= state.messages.length;
}

async function loadEarlierMessages() {
  const element = transcript.value;
  if (!element || transcriptWindowStart.value === 0) return;
  shiftingWindow.value = true;
  const previousHeight = element.scrollHeight;
  transcriptWindowStart.value = olderWindowStart(transcriptWindowStart.value);
  await nextTick();
  element.scrollTop = element.scrollHeight - previousHeight + 1;
  pinnedToBottom.value = false;
  shiftingWindow.value = false;
}

async function loadNewerMessages() {
  const element = transcript.value;
  if (!element || transcriptWindowEndIndex.value >= state.messages.length)
    return;
  shiftingWindow.value = true;
  transcriptWindowStart.value = newerWindowStart(
    transcriptWindowStart.value,
    state.messages,
  );
  await nextTick();
  element.scrollTop = 1;
  shiftingWindow.value = false;
}

function handleComposerKeydown(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    void sendMessage();
  }
}

function handleModelChange(event: Event) {
  void selectModel((event.target as HTMLSelectElement).value);
}

function handleEffortChange(event: Event) {
  void selectEffort((event.target as HTMLSelectElement).value as ThinkingLevel);
}

function handleNewSession() {
  if (activeProject.value) void newSession(activeProject.value);
}
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <header class="sidebar-titlebar" data-tauri-drag-region></header>

      <div class="project-list">
        <template
          v-for="project in state.workspace?.projects"
          :key="project.path"
        >
          <div class="project-row" :class="{ selected: project.selected }">
            <button
              class="project-toggle"
              type="button"
              :title="project.path"
              @click="toggleProject(project)"
            >
              <span>{{ project.name }}</span>
              <UiIcon
                name="chevron"
                :class="{ expanded: !project.collapsed }"
              />
            </button>
            <button
              class="row-action new-session-action"
              type="button"
              :aria-label="`New session in ${project.name}`"
              title="New session"
              @click="newSession(project)"
            >
              <UiIcon name="plus" />
            </button>
            <button
              class="row-action danger"
              type="button"
              :aria-label="`Remove ${project.name}`"
              title="Remove project"
              @click="removeProject(project)"
            >
              <UiIcon name="trash" />
            </button>
          </div>

          <div v-if="!project.collapsed" class="session-list">
            <button
              v-for="session in project.sessions.filter(
                (item) => !item.archived,
              )"
              :key="session.id"
              class="session-row"
              :class="{ selected: session.id === state.activeSessionId }"
              type="button"
              @click="selectSession(project, session)"
            >
              <span class="session-copy">
                <span class="session-title">{{ session.title }}</span>
                <span class="session-time">{{ session.lastActive }}</span>
              </span>
            </button>
            <div v-if="project.sessions.length === 0" class="empty-sessions">
              No sessions yet
            </div>
          </div>
        </template>

        <div
          v-if="state.workspace && state.workspace.projects.length === 0"
          class="empty-projects"
        >
          <UiIcon name="folder" />
          <span>No projects yet</span>
        </div>
      </div>

      <footer class="sidebar-footer">
        <button
          class="icon-button"
          type="button"
          title="Add project"
          aria-label="Add project"
          @click="addProject"
        >
          <UiIcon name="folder" />
        </button>
      </footer>
    </aside>

    <main class="session-pane">
      <header class="session-header" data-tauri-drag-region>
        <div class="session-heading" data-tauri-drag-region>
          <h1>{{ sessionTitle }}</h1>
        </div>
        <button
          v-if="activeProject"
          class="icon-button session-new-button"
          type="button"
          title="New session"
          aria-label="New session"
          :disabled="
            state.streaming ||
            state.stopping ||
            state.startingSession ||
            state.switchingSession
          "
          @click="handleNewSession"
        >
          <UiIcon name="plus" />
        </button>
        <span
          v-if="state.switchingSession || state.startingSession"
          class="spinner"
          aria-label="Loading"
        ></span>
      </header>

      <section
        ref="transcript"
        class="transcript"
        aria-label="Tau transcript"
        @scroll="handleTranscriptScroll"
      >
        <div v-if="state.messages.length" class="message-list">
          <button
            v-if="transcriptWindowStart > 0"
            class="transcript-boundary"
            type="button"
            @click="loadEarlierMessages"
          >
            Load earlier messages
          </button>
          <article
            v-for="message in visibleMessages"
            :key="message.id"
            class="message"
            :class="message.kind"
          >
            <div v-if="message.kind === 'user'" class="user-bubble">
              {{ message.text }}
            </div>
            <MarkdownText
              v-else-if="message.kind === 'assistant'"
              :source="message.text"
            />
            <div v-else-if="message.kind === 'thinking'" class="thinking-block">
              <div class="thinking-label">Thinking</div>
              <MarkdownText :source="message.text" />
            </div>
            <div
              v-else
              class="tool-row"
              :class="{ error: message.toolErrored }"
            >
              <span class="tool-copy">
                <span v-if="message.text" class="tool-argument">{{
                  message.text
                }}</span>
                <span class="tool-name">{{ message.toolName || "tool" }}</span>
              </span>
              <span
                v-if="message.toolRunning"
                class="spinner small"
                aria-label="Running"
              ></span>
              <span v-else-if="message.toolErrored" class="tool-status"
                >Failed</span
              >
            </div>
          </article>

          <button
            v-if="transcriptWindowEndIndex < state.messages.length"
            class="transcript-boundary"
            type="button"
            @click="loadNewerMessages"
          >
            Load newer messages
          </button>

          <div v-if="state.stopping || state.streaming" class="stream-state">
            <PiSpinner
              :label="state.stopping ? 'Pi is stopping' : 'Pi is working'"
            />
          </div>
        </div>
      </section>

      <footer class="composer-area">
        <p v-if="state.status" class="status" role="status">
          {{ state.status }}
        </p>
        <div class="composer" :class="{ disabled: !state.piReady }">
          <textarea
            v-model="state.draft"
            rows="2"
            maxlength="32768"
            placeholder="Message π"
            :disabled="!state.piReady"
            aria-label="Message Pi"
            @keydown="handleComposerKeydown"
          ></textarea>
          <div class="composer-toolbar">
            <select
              :value="`${state.currentModelProvider}/${state.currentModelId}`"
              :disabled="settingsDisabled || state.models.length === 0"
              aria-label="Model"
              @change="handleModelChange"
            >
              <option v-if="!state.currentModelId" value="/">Model</option>
              <option
                v-for="model in state.models"
                :key="`${model.provider}/${model.id}`"
                :value="`${model.provider}/${model.id}`"
              >
                {{ model.name }} · {{ model.provider }}
              </option>
            </select>
            <select
              :value="state.currentEffort"
              :disabled="settingsDisabled || state.efforts.length === 0"
              aria-label="Thinking effort"
              @change="handleEffortChange"
            >
              <option
                v-for="effort in state.efforts"
                :key="effort"
                :value="effort"
              >
                {{ effortLabels[effort] }}
              </option>
            </select>
            <button
              v-if="state.streaming"
              class="send-button stop"
              type="button"
              :disabled="state.stopping"
              aria-label="Stop Pi"
              @click="stop"
            >
              <UiIcon name="stop" />
            </button>
            <button
              v-else
              class="send-button"
              type="button"
              :disabled="!state.piReady || !state.draft.trim()"
              aria-label="Send message"
              @click="sendMessage"
            >
              <UiIcon name="send" />
            </button>
          </div>
        </div>
      </footer>
    </main>
  </div>
</template>
