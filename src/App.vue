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
  commandInvocation,
  filterCommands,
  slashCommandQuery,
} from "./lib/commands";
import {
  latestWindowStart,
  newerWindowStart,
  olderWindowStart,
  transcriptWindowEnd,
} from "./lib/transcript-window";
import type { CommandOption, ThinkingLevel } from "./types";

const SIDEBAR_WIDTH_STORAGE_KEY = "tau.sidebar-width";
const DEFAULT_SIDEBAR_WIDTH = 260;
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 480;

const transcript = ref<HTMLElement>();
const composerInput = ref<HTMLTextAreaElement>();
const projectMenu = ref<HTMLElement>();
const sidebar = ref<HTMLElement>();
const remoteConnectionInput = ref<HTMLInputElement>();
const remoteDirectoryFilterInput = ref<HTMLInputElement>();
const extensionDialogInput = ref<HTMLInputElement | HTMLTextAreaElement>();
const extensionDialogPrimaryAction = ref<HTMLButtonElement>();
const projectMenuOpen = ref(false);
const commandSelectedIndex = ref(0);
const extensionDialogSelectedIndex = ref(0);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
const pinnedToBottom = ref(true);
const transcriptWindowStart = ref(0);
const shiftingWindow = ref(false);
const {
  state,
  activeProject,
  messages,
  draft,
  status,
  streaming,
  stopping,
  models,
  efforts,
  commands,
  activeExtensionDialog,
  extensionNotifications,
  currentModelProvider,
  currentModelId,
  currentEffort,
  sessionTitle,
  currentModelLabel,
  currentEffortLabel,
  effortLabels,
  settingsDisabled,
  canDraft,
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
  archiveSession,
  newSession,
  selectSession,
  canArchiveSession,
  projectSessions,
  sessionLastActive,
  isSessionSelected,
  sessionIndicator,
  sendMessage,
  stop,
  submitExtensionDialog,
  cancelExtensionDialog,
  dismissExtensionNotification,
  selectModel,
  selectEffort,
} = useTau();

const transcriptWindowEndIndex = computed(() =>
  transcriptWindowEnd(messages.value, transcriptWindowStart.value),
);
const visibleMessages = computed(() =>
  messages.value.slice(
    transcriptWindowStart.value,
    transcriptWindowEndIndex.value,
  ),
);
const sessionIsEmpty = computed(
  () =>
    canDraft.value &&
    !messages.value.some(
      (message) => message.kind === "user" || message.kind === "assistant",
    ),
);
const showWorkingIndicator = computed(() => {
  if (stopping.value) return true;
  if (!streaming.value) return false;
  return messages.value[messages.value.length - 1]?.kind !== "assistant";
});
const commandQuery = computed(() => slashCommandQuery(draft.value));
const commandMenuActive = computed(
  () => canDraft.value && commandQuery.value !== null,
);
const filteredCommands = computed(() =>
  commandQuery.value === null
    ? []
    : filterCommands(commands.value, commandQuery.value),
);
const selectedCommand = computed(
  () => filteredCommands.value[commandSelectedIndex.value],
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
  void nextTick(resizeComposer);
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

watch(activeExtensionDialog, (dialog) => {
  extensionDialogSelectedIndex.value = 0;
  void nextTick(() => {
    if (!dialog) {
      composerInput.value?.focus();
    } else if (dialog.method === "select") {
      document.getElementById("extension-dialog-option-0")?.focus();
    } else if (dialog.method === "confirm") {
      extensionDialogPrimaryAction.value?.focus();
    } else {
      extensionDialogInput.value?.focus();
    }
  });
});

watch(
  () => state.activeControllerKey,
  (controllerKey) => {
    if (!controllerKey) return;
    void nextTick(() => {
      if (!state.remoteDialogOpen && !activeExtensionDialog.value) {
        composerInput.value?.focus();
      }
    });
  },
);

watch([commandQuery, commands], () => {
  commandSelectedIndex.value = 0;
});

watch([draft, sessionIsEmpty], () => {
  void nextTick(resizeComposer);
});

watch(messages, () => {
  transcriptWindowStart.value = latestWindowStart(messages.value);
  pinnedToBottom.value = true;
});

watch(
  () => [
    messages.value.length,
    messages.value[messages.value.length - 1]?.text,
  ],
  () => {
    if (!pinnedToBottom.value) return;
    transcriptWindowStart.value = latestWindowStart(messages.value);
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
  if (nearBottom && transcriptWindowEndIndex.value < messages.value.length) {
    await loadNewerMessages();
    return;
  }
  pinnedToBottom.value =
    nearBottom && transcriptWindowEndIndex.value >= messages.value.length;
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
  if (!element || transcriptWindowEndIndex.value >= messages.value.length)
    return;
  shiftingWindow.value = true;
  transcriptWindowStart.value = newerWindowStart(
    transcriptWindowStart.value,
    messages.value,
  );
  await nextTick();
  element.scrollTop = 1;
  shiftingWindow.value = false;
}

function resizeComposer() {
  const element = composerInput.value;
  if (!element) return;
  element.style.height = "auto";
  if (sessionIsEmpty.value) {
    element.style.overflowY = "auto";
    return;
  }
  const maxHeight = Number.parseFloat(getComputedStyle(element).maxHeight);
  const height = Number.isFinite(maxHeight)
    ? Math.min(element.scrollHeight, maxHeight)
    : element.scrollHeight;
  element.style.height = `${Math.ceil(height)}px`;
  element.style.overflowY =
    Number.isFinite(maxHeight) && element.scrollHeight > maxHeight
      ? "auto"
      : "hidden";
}

function handleComposerKeydown(event: KeyboardEvent) {
  if (commandMenuActive.value && !event.isComposing) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const lastIndex = filteredCommands.value.length - 1;
      if (lastIndex < 0) return;
      const delta = event.key === "ArrowDown" ? 1 : -1;
      commandSelectedIndex.value = Math.min(
        lastIndex,
        Math.max(0, commandSelectedIndex.value + delta),
      );
      scrollSelectedCommand();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (selectedCommand.value) executeCommand(selectedCommand.value);
      return;
    }
  }

  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    handleSendMessage();
  }
}

function handleSendMessage() {
  pinnedToBottom.value = true;
  void sendMessage();
}

function executeCommand(command: CommandOption) {
  draft.value = commandInvocation(command);
  commandSelectedIndex.value = 0;
  handleSendMessage();
}

function scrollSelectedCommand() {
  void nextTick(() => {
    document
      .getElementById(`command-option-${commandSelectedIndex.value}`)
      ?.scrollIntoView({ block: "nearest" });
  });
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

function startSidebarResize(event: PointerEvent) {
  if (event.button !== 0) return;
  const handle = event.currentTarget;
  if (!(handle instanceof HTMLElement)) return;

  event.preventDefault();
  resizingSidebar.value = true;
  handle.setPointerCapture(event.pointerId);
  handle.focus({ preventScroll: true });
  updateSidebarWidth(event.clientX);
}

function handleSidebarResize(event: PointerEvent) {
  if (resizingSidebar.value) updateSidebarWidth(event.clientX);
}

function finishSidebarResize(event: PointerEvent) {
  if (!resizingSidebar.value) return;
  updateSidebarWidth(event.clientX);
  stopSidebarResize();
}

function stopSidebarResize() {
  if (!resizingSidebar.value) return;
  resizingSidebar.value = false;
  persistSidebarWidth();
}

function handleSidebarResizeKeydown(event: KeyboardEvent) {
  const step = event.shiftKey ? 40 : 10;
  let nextWidth: number;

  switch (event.key) {
    case "ArrowLeft":
      nextWidth = sidebarWidth.value - step;
      break;
    case "ArrowRight":
      nextWidth = sidebarWidth.value + step;
      break;
    case "Home":
      nextWidth = MIN_SIDEBAR_WIDTH;
      break;
    case "End":
      nextWidth = MAX_SIDEBAR_WIDTH;
      break;
    default:
      return;
  }

  event.preventDefault();
  sidebarWidth.value = clampSidebarWidth(nextWidth);
  persistSidebarWidth();
}

function updateSidebarWidth(pointerX: number) {
  const left = sidebar.value?.getBoundingClientRect().left ?? 0;
  sidebarWidth.value = clampSidebarWidth(pointerX - left);
}

function persistSidebarWidth() {
  try {
    localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(sidebarWidth.value));
  } catch {
    return;
  }
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
  if (
    event.metaKey &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.shiftKey &&
    !event.repeat &&
    event.key.toLowerCase() === "n"
  ) {
    event.preventDefault();
    handleNewSession();
    return;
  }
  if (event.key !== "Escape") return;
  if (activeExtensionDialog.value) {
    event.preventDefault();
    void cancelExtensionDialog();
  } else if (state.remoteDialogOpen) closeRemoteProjectDialog();
  else projectMenuOpen.value = false;
}

function chooseExtensionDialogOption(value: string) {
  void submitExtensionDialog(value);
}

function respondToExtensionConfirmation(confirmed: boolean) {
  void submitExtensionDialog(confirmed);
}

function handleExtensionDialogSubmit() {
  const dialog = activeExtensionDialog.value;
  if (!dialog || (dialog.method !== "input" && dialog.method !== "editor")) {
    return;
  }
  void submitExtensionDialog(dialog.draft);
}

function handleExtensionSelectKeydown(event: KeyboardEvent) {
  const dialog = activeExtensionDialog.value;
  const options = dialog?.options ?? [];
  if (dialog?.method !== "select" || options.length === 0) return;
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const delta = event.key === "ArrowDown" ? 1 : -1;
    extensionDialogSelectedIndex.value =
      (extensionDialogSelectedIndex.value + delta + options.length) %
      options.length;
    document
      .getElementById(
        `extension-dialog-option-${extensionDialogSelectedIndex.value}`,
      )
      ?.focus();
  }
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

function loadSidebarWidth(): number {
  try {
    const storedWidth = Number.parseFloat(
      localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? "",
    );
    if (Number.isFinite(storedWidth)) return clampSidebarWidth(storedWidth);
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
  return DEFAULT_SIDEBAR_WIDTH;
}

function clampSidebarWidth(width: number): number {
  return Math.min(
    MAX_SIDEBAR_WIDTH,
    Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)),
  );
}
</script>

<template>
  <div
    class="app-shell"
    :class="{ 'resizing-sidebar': resizingSidebar }"
    :style="{ '--sidebar-width': `${sidebarWidth}px` }"
  >
    <aside ref="sidebar" class="sidebar">
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
            <div
              v-for="session in projectSessions(project)"
              :key="session.id"
              class="session-row"
              :class="{
                selected: isSessionSelected(project, session),
                archivable: canArchiveSession(project, session),
              }"
            >
              <button
                class="session-select"
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
                  <span class="session-time">{{
                    sessionLastActive(project, session)
                  }}</span>
                </span>
              </button>
              <button
                v-if="canArchiveSession(project, session)"
                class="session-archive"
                type="button"
                :aria-label="`Archive ${session.title}`"
                title="Archive session"
                @click="archiveSession(project, session)"
              >
                <UiIcon name="archive" />
              </button>
            </div>
            <div
              v-if="projectSessions(project).length === 0"
              class="empty-sessions"
            >
              No active sessions
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

      <div
        class="sidebar-resize-handle"
        role="separator"
        aria-label="Resize sidebar"
        aria-orientation="vertical"
        :aria-valuemin="MIN_SIDEBAR_WIDTH"
        :aria-valuemax="MAX_SIDEBAR_WIDTH"
        :aria-valuenow="sidebarWidth"
        tabindex="0"
        @pointerdown="startSidebarResize"
        @pointermove="handleSidebarResize"
        @pointerup="finishSidebarResize"
        @pointercancel="stopSidebarResize"
        @lostpointercapture="stopSidebarResize"
        @keydown="handleSidebarResizeKeydown"
      ></div>
    </aside>

    <main class="session-pane" :class="{ 'empty-session': sessionIsEmpty }">
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
          @click="handleNewSession"
        >
          <UiIcon name="plus" />
        </button>
      </header>

      <section
        v-if="!sessionIsEmpty"
        ref="transcript"
        class="transcript"
        aria-label="Tau transcript"
        @scroll="handleTranscriptScroll"
      >
        <div v-if="messages.length" class="message-list">
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
                class="tool-running-indicator"
                role="status"
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
            v-if="transcriptWindowEndIndex < messages.length"
            class="transcript-boundary"
            type="button"
            @click="loadNewerMessages"
          >
            Load newer messages
          </button>

          <div v-if="showWorkingIndicator" class="stream-state">
            <PiSpinner :label="stopping ? 'Pi is stopping' : 'Pi is working'" />
          </div>
        </div>
      </section>

      <footer class="composer-area">
        <p v-if="status" class="status" role="status">
          {{ status }}
        </p>
        <form
          v-if="activeExtensionDialog"
          class="extension-composer"
          role="dialog"
          aria-modal="false"
          :aria-label="activeExtensionDialog.title"
          @submit.prevent="handleExtensionDialogSubmit"
        >
          <header class="extension-dialog-header">
            <span class="extension-dialog-context">
              {{ activeExtensionDialog.projectName }} ·
              {{ activeExtensionDialog.sessionName }}
            </span>
            <h2>{{ activeExtensionDialog.title }}</h2>
          </header>

          <p
            v-if="activeExtensionDialog.message"
            class="extension-dialog-message"
          >
            {{ activeExtensionDialog.message }}
          </p>

          <div
            v-if="activeExtensionDialog.method === 'select'"
            class="extension-dialog-options"
            role="listbox"
            @keydown="handleExtensionSelectKeydown"
          >
            <button
              v-for="(option, index) in activeExtensionDialog.options"
              :id="`extension-dialog-option-${index}`"
              :key="`${index}:${option}`"
              class="extension-dialog-option"
              :class="{ selected: index === extensionDialogSelectedIndex }"
              type="button"
              role="option"
              :aria-selected="index === extensionDialogSelectedIndex"
              @mouseenter="extensionDialogSelectedIndex = index"
              @click="chooseExtensionDialogOption(option)"
            >
              {{ option }}
            </button>
            <div
              v-if="activeExtensionDialog.options?.length === 0"
              class="extension-dialog-empty"
            >
              No options available
            </div>
          </div>

          <input
            v-else-if="activeExtensionDialog.method === 'input'"
            ref="extensionDialogInput"
            v-model="activeExtensionDialog.draft"
            class="extension-dialog-input"
            type="text"
            autocomplete="off"
            :placeholder="activeExtensionDialog.placeholder"
            :aria-label="activeExtensionDialog.title"
          />

          <textarea
            v-else-if="activeExtensionDialog.method === 'editor'"
            ref="extensionDialogInput"
            v-model="activeExtensionDialog.draft"
            class="extension-dialog-editor"
            rows="6"
            :aria-label="activeExtensionDialog.title"
            @keydown.meta.enter.prevent="handleExtensionDialogSubmit"
            @keydown.ctrl.enter.prevent="handleExtensionDialogSubmit"
          ></textarea>

          <footer class="extension-dialog-actions">
            <template v-if="activeExtensionDialog.method === 'confirm'">
              <button
                ref="extensionDialogPrimaryAction"
                class="extension-dialog-button primary"
                type="button"
                @click="respondToExtensionConfirmation(true)"
              >
                Confirm
              </button>
              <button
                class="extension-dialog-button secondary"
                type="button"
                @click="respondToExtensionConfirmation(false)"
              >
                No
              </button>
            </template>
            <button
              v-else-if="
                activeExtensionDialog.method === 'input' ||
                activeExtensionDialog.method === 'editor'
              "
              class="extension-dialog-button primary"
              type="submit"
            >
              Submit
            </button>
            <button
              class="extension-dialog-button secondary"
              type="button"
              @click="cancelExtensionDialog"
            >
              Cancel
            </button>
          </footer>
        </form>
        <div v-else class="composer" :class="{ disabled: !canDraft }">
          <div
            v-if="commandMenuActive"
            id="command-menu"
            class="command-menu"
            role="listbox"
            aria-label="Commands"
          >
            <button
              v-for="(command, index) in filteredCommands"
              :id="`command-option-${index}`"
              :key="`${command.source}:${command.name}`"
              class="command-option"
              :class="{ selected: index === commandSelectedIndex }"
              type="button"
              role="option"
              tabindex="-1"
              :aria-selected="index === commandSelectedIndex"
              @mousedown.prevent
              @mouseenter="commandSelectedIndex = index"
              @click="executeCommand(command)"
            >
              <span class="command-copy">
                <span class="command-name">/{{ command.name }}</span>
                <span v-if="command.description" class="command-description">
                  {{ command.description }}
                </span>
              </span>
              <span class="command-source">{{ command.source }}</span>
            </button>
            <div v-if="filteredCommands.length === 0" class="empty-commands">
              No matching commands
            </div>
          </div>
          <textarea
            ref="composerInput"
            v-model="draft"
            rows="4"
            maxlength="32768"
            placeholder="Message π"
            :disabled="!canDraft"
            :aria-expanded="commandMenuActive"
            :aria-controls="commandMenuActive ? 'command-menu' : undefined"
            :aria-activedescendant="
              commandMenuActive && selectedCommand
                ? `command-option-${commandSelectedIndex}`
                : undefined
            "
            aria-autocomplete="list"
            aria-label="Message Pi"
            @keydown="handleComposerKeydown"
          ></textarea>
          <div class="composer-toolbar">
            <span class="composer-selector model-selector">
              <select
                :value="`${currentModelProvider}/${currentModelId}`"
                :disabled="settingsDisabled || models.length === 0"
                aria-label="Model"
                @change="handleModelChange"
              >
                <option v-if="!currentModelId" value="/">Model</option>
                <option
                  v-else-if="
                    !models.some(
                      (model) =>
                        model.provider === currentModelProvider &&
                        model.id === currentModelId,
                    )
                  "
                  :value="`${currentModelProvider}/${currentModelId}`"
                >
                  {{ currentModelLabel }}
                </option>
                <option
                  v-for="model in models"
                  :key="`${model.provider}/${model.id}`"
                  :value="`${model.provider}/${model.id}`"
                >
                  {{ model.name }} · {{ model.provider }}
                </option>
              </select>
            </span>
            <span class="composer-selector effort-selector">
              <select
                :value="currentEffort"
                :disabled="settingsDisabled || efforts.length === 0"
                aria-label="Thinking effort"
                @change="handleEffortChange"
              >
                <option
                  v-if="!efforts.includes(currentEffort)"
                  :value="currentEffort"
                >
                  {{ currentEffortLabel }}
                </option>
                <option v-for="effort in efforts" :key="effort" :value="effort">
                  {{ effortLabels[effort] }}
                </option>
              </select>
            </span>
            <button
              v-if="streaming"
              class="send-button stop"
              type="button"
              :disabled="stopping"
              aria-label="Stop Pi"
              @click="stop"
            >
              <UiIcon name="stop" />
            </button>
            <button
              v-else
              class="send-button"
              type="button"
              :disabled="!canCompose || !draft.trim()"
              aria-label="Send message"
              @click="handleSendMessage"
            >
              <UiIcon name="triangle" />
            </button>
          </div>
        </div>
      </footer>
    </main>

    <div
      v-if="extensionNotifications.length"
      class="extension-notification-stack"
      aria-live="polite"
    >
      <div
        v-for="notification in extensionNotifications"
        :key="notification.key"
        class="extension-notification"
        :class="notification.type"
      >
        <div class="extension-notification-copy">
          <div class="extension-notification-header">
            <span class="extension-notification-context">
              {{ notification.projectName }} · {{ notification.sessionName }}
            </span>
            <button
              type="button"
              aria-label="Dismiss notification"
              @click="dismissExtensionNotification(notification.key)"
            >
              <UiIcon name="cross" />
            </button>
          </div>
          <span class="extension-notification-message">{{
            notification.message
          }}</span>
        </div>
      </div>
    </div>

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
