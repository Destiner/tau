<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
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
const projectMenu = ref<HTMLElement>();
const remoteConnectionInput = ref<HTMLInputElement>();
const remoteDirectoryFilterInput = ref<HTMLInputElement>();
const projectMenuOpen = ref(false);
const pinnedToBottom = ref(true);
const transcriptWindowStart = ref(0);
const shiftingWindow = ref(false);
const {
  state,
  activeProject,
  sessionTitle,
  effortLabels,
  settingsDisabled,
  canCompose,
  initialize,
  dispose,
  addLocalProject,
  openRemoteProjectDialog,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  toggleProject,
  removeProject,
  newSession,
  selectSession,
  projectSessions,
  isSessionSelected,
  sessionIndicator,
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
const remoteDirectoryOptions = computed(() => {
  if (!state.remoteWorkingDirectory) return [];
  const options = [] as Array<{
    name: string;
    path: string;
    kind: "back" | "select" | "forward";
  }>;
  const previousDirectory =
    state.remoteWorkingDirectory === state.remoteDirectoryRoot
      ? undefined
      : state.remoteDirectoryHistory[state.remoteDirectoryHistory.length - 1];
  if (previousDirectory) {
    options.push({
      name: "Go Back",
      path: previousDirectory,
      kind: "back",
    });
  }
  options.push({
    name: `Select ${state.remoteWorkingDirectory}`,
    path: state.remoteWorkingDirectory,
    kind: "select",
  });
  options.push(
    ...state.remoteDirectories.map((directory) => ({
      ...directory,
      kind: "forward" as const,
    })),
  );

  const filter = state.remoteDirectoryFilter.trim().toLocaleLowerCase();
  return options.filter((option) => {
    const name = option.name.toLocaleLowerCase();
    if (option.kind === "forward" && option.name.startsWith(".")) {
      return filter === name;
    }
    return !filter || name.includes(filter);
  });
});

onMounted(() => {
  void initialize();
  document.addEventListener("pointerdown", handleDocumentPointerDown);
  document.addEventListener("keydown", handleDocumentKeydown);
});
onBeforeUnmount(() => {
  dispose();
  document.removeEventListener("pointerdown", handleDocumentPointerDown);
  document.removeEventListener("keydown", handleDocumentKeydown);
});

watch(
  () => [state.remoteDialogOpen, state.remoteDialogStep] as const,
  ([open, step]) => {
    if (!open) return;
    void nextTick(() => {
      if (step === "connection") remoteConnectionInput.value?.focus();
      else remoteDirectoryFilterInput.value?.focus();
    });
  },
);

watch(
  () => state.remoteDirectoryFilter,
  () => {
    state.remoteDirectorySelectedIndex = 0;
  },
);

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

function handleDocumentPointerDown(event: PointerEvent) {
  const target = event.target;
  if (
    projectMenuOpen.value &&
    target instanceof Node &&
    !projectMenu.value?.contains(target)
  ) {
    projectMenuOpen.value = false;
  }
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  if (state.remoteDialogOpen) closeRemoteProjectDialog();
  else projectMenuOpen.value = false;
}

function handleLocalProject() {
  projectMenuOpen.value = false;
  void addLocalProject();
}

function handleRemoteProject() {
  projectMenuOpen.value = false;
  openRemoteProjectDialog();
}

function handleRemoteDirectoryKeydown(event: KeyboardEvent) {
  if (state.remoteConnecting) return;
  const lastIndex = remoteDirectoryOptions.value.length - 1;
  if (lastIndex < 0) return;
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const delta = event.key === "ArrowDown" ? 1 : -1;
    state.remoteDirectorySelectedIndex = Math.min(
      lastIndex,
      Math.max(0, state.remoteDirectorySelectedIndex + delta),
    );
    scrollSelectedRemoteDirectory();
    return;
  }
  if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    const option =
      remoteDirectoryOptions.value[state.remoteDirectorySelectedIndex];
    if (option) void chooseRemoteDirectory(option.path, option.kind);
  }
}

function scrollSelectedRemoteDirectory() {
  void nextTick(() => {
    document
      .getElementById(
        `remote-directory-option-${state.remoteDirectorySelectedIndex}`,
      )
      ?.scrollIntoView({ block: "nearest" });
  });
}

function handleTitlebarMouseDown(event: MouseEvent) {
  if (event.button !== 0) return;
  const target = event.target;
  if (
    !(target instanceof Element) ||
    target.closest("button, a, input, select, textarea")
  ) {
    return;
  }
  void getCurrentWindow()
    .startDragging()
    .catch(() => undefined);
}
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <header
        class="sidebar-titlebar"
        @mousedown="handleTitlebarMouseDown"
      ></header>

      <div class="project-list">
        <template
          v-for="project in state.workspace?.projects"
          :key="project.path"
        >
          <div
            class="project-row"
            :class="{ selected: project.path === state.activeProjectPath }"
          >
            <button
              class="project-toggle"
              type="button"
              :title="
                project.connectionString
                  ? `${project.connectionString} · ${project.workingDirectory}`
                  : project.workingDirectory
              "
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
              v-for="session in projectSessions(project)"
              :key="session.id"
              class="session-row"
              :class="{ selected: isSessionSelected(project, session) }"
              type="button"
              @click="selectSession(project, session)"
            >
              <span
                class="session-indicator"
                :class="sessionIndicator(project, session)"
                aria-hidden="true"
              ></span>
              <span class="session-copy">
                <span class="session-title">{{ session.title }}</span>
                <span class="session-time">{{ session.lastActive }}</span>
              </span>
            </button>
            <div
              v-if="projectSessions(project).length === 0"
              class="empty-sessions"
            >
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
        <div ref="projectMenu" class="project-menu-wrap">
          <button
            class="icon-button"
            type="button"
            title="Open project"
            aria-label="Open project"
            aria-haspopup="menu"
            :aria-expanded="projectMenuOpen"
            @click="projectMenuOpen = !projectMenuOpen"
          >
            <UiIcon name="folder" />
          </button>
          <div v-if="projectMenuOpen" class="project-menu" role="menu">
            <button type="button" role="menuitem" @click="handleLocalProject">
              Open Local Project
            </button>
            <button type="button" role="menuitem" @click="handleRemoteProject">
              Open Remote Project
            </button>
          </div>
        </div>
      </footer>
    </aside>

    <main class="session-pane">
      <header class="session-header" @mousedown="handleTitlebarMouseDown">
        <div class="session-heading">
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
                <span class="tool-name">{{ message.toolName || "tool" }}</span>
                <span v-if="message.text" class="tool-argument">{{
                  message.text
                }}</span>
              </span>
              <span
                v-if="message.toolRunning"
                class="spinner small"
                aria-label="Running"
              ></span>
              <UiIcon
                v-else-if="message.toolErrored"
                class="tool-error-icon"
                name="cross"
                aria-label="Failed"
              />
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
        <div class="composer" :class="{ disabled: !canCompose }">
          <textarea
            v-model="state.draft"
            rows="2"
            maxlength="32768"
            placeholder="Message π"
            :disabled="!canCompose"
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
              :disabled="!canCompose || !state.draft.trim()"
              aria-label="Send message"
              @click="sendMessage"
            >
              <UiIcon name="send" />
            </button>
          </div>
        </div>
      </footer>
    </main>

    <div
      v-if="state.remoteDialogOpen"
      class="dialog-layer"
      @mousedown.self="closeRemoteProjectDialog"
    >
      <form
        v-if="state.remoteDialogStep === 'connection'"
        class="remote-dialog remote-connection-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="SSH connection"
        :aria-busy="state.remoteConnecting"
        @submit.prevent="submitRemoteConnection"
      >
        <input
          ref="remoteConnectionInput"
          v-model="state.remoteConnectionString"
          :class="{ error: state.remoteConnectionError }"
          type="text"
          inputmode="text"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="ssh user@example -p 1234"
          :aria-label="state.remoteConnectionError || 'SSH connection string'"
          :aria-invalid="Boolean(state.remoteConnectionError)"
          :title="state.remoteConnectionError"
          :readonly="
            state.remoteConnecting || state.remoteDialogMode === 'retry'
          "
        />
      </form>

      <div
        v-else
        class="remote-dialog remote-directory-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Choose remote working directory"
        :aria-busy="state.remoteConnecting"
      >
        <input
          ref="remoteDirectoryFilterInput"
          v-model="state.remoteDirectoryFilter"
          :class="{ error: state.remoteConnectionError }"
          type="text"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          placeholder="Filter directories"
          aria-label="Filter remote directories"
          aria-controls="remote-directory-list"
          :aria-activedescendant="`remote-directory-option-${state.remoteDirectorySelectedIndex}`"
          :aria-invalid="Boolean(state.remoteConnectionError)"
          :title="state.remoteConnectionError"
          :readonly="state.remoteConnecting"
          @keydown="handleRemoteDirectoryKeydown"
        />
        <div
          id="remote-directory-list"
          class="remote-directory-list"
          role="listbox"
        >
          <button
            v-for="(option, index) in remoteDirectoryOptions"
            :id="`remote-directory-option-${index}`"
            :key="option.path"
            class="remote-directory-option"
            :class="{
              selected: index === state.remoteDirectorySelectedIndex,
            }"
            type="button"
            role="option"
            tabindex="-1"
            :aria-selected="index === state.remoteDirectorySelectedIndex"
            :disabled="state.remoteConnecting"
            :title="option.path"
            @mousedown.prevent
            @mouseenter="state.remoteDirectorySelectedIndex = index"
            @click="chooseRemoteDirectory(option.path, option.kind)"
          >
            {{ option.name }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
