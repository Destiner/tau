<script setup lang="ts">
import { type UnlistenFn, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import Sortable, { type SortableEvent } from "sortablejs";
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
import TranscriptView from "./components/TranscriptView.vue";
import UiIcon from "./components/UiIcon.vue";
import { type SessionIndicator, useTau } from "./composables/useTau";
import {
  type CommandMenuPlacement,
  commandInvocation,
  commandMenuLayout,
  filterCommands,
  slashCommandQuery,
} from "./lib/commands";
import type {
  CommandOption,
  ProjectSummary,
  SessionSummary,
  ThinkingLevel,
} from "./types";

type TextField = HTMLInputElement | HTMLTextAreaElement;

interface ContextMenuItem {
  label: string;
  disabled?: boolean;
  run: () => void;
}

interface ContextMenuState {
  items: ContextMenuItem[];
  x: number;
  y: number;
}

const SIDEBAR_WIDTH_STORAGE_KEY = "tau.sidebar-width";
const CONTEXT_MENU_MARGIN = 8;
const NEW_SESSION_EVENT = "tau://new-session";
/** How long a session may hydrate before it is worth reporting as loading. */
const LOADING_INDICATOR_DELAY_MS = 200;
// The rename field applies its value the moment it loses focus, so it is left
// out: a menu that takes focus to open would end the rename it edits.
const TEXT_FIELD_SELECTOR = "input:not(.session-name-input), textarea";
const EDITABLE_SELECTOR = "input, textarea, select";
const DEFAULT_SIDEBAR_WIDTH = 260;
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 480;

const transcriptView = ref<InstanceType<typeof TranscriptView>>();
const sessionHeader = ref<HTMLElement>();
const composer = ref<HTMLElement>();
const composerInput = ref<HTMLTextAreaElement>();
const commandMenu = ref<HTMLElement>();
const projectMenu = ref<HTMLElement>();
const contextMenu = ref<HTMLElement>();
const projectList = ref<HTMLElement>();
const sidebar = ref<HTMLElement>();
const remoteConnectionInput = ref<HTMLInputElement>();
const remoteDirectoryFilterInput = ref<HTMLInputElement>();
const extensionDialogInput = ref<HTMLInputElement | HTMLTextAreaElement>();
const extensionDialogPrimaryAction = ref<HTMLButtonElement>();
const projectMenuOpen = ref(false);
const contextMenuState = ref<ContextMenuState>();
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const commandMenuDismissed = ref(false);
const commandSelectedIndex = ref(0);
const commandMenuPlacement = ref<CommandMenuPlacement>("above");
const commandMenuMaxHeight = ref<number>();
const commandMenuOffset = ref(0);
const extensionDialogSelectedIndex = ref(0);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
const sessionTitleInput = ref<HTMLInputElement>();
const renamingSession = ref(false);
const sessionNameDraft = ref("");
let projectSortable: Sortable | undefined;
let unlistenWindowFocus: UnlistenFn | undefined;
let unlistenNewSessionMenu: UnlistenFn | undefined;
let loadingIndicatorTimer: ReturnType<typeof setTimeout> | undefined;
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
  canRenameSession,
  sessionLoading,
  initialize,
  dispose,
  addLocalProject,
  openRemoteProjectDialog,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  toggleProject,
  reorderProjects,
  removeProject,
  archiveSession,
  newSession,
  selectSession,
  canArchiveSession,
  projectSessions,
  sessionLastActive,
  isSessionSelected,
  sessionIndicator,
  isSessionUnread,
  markSessionUnread,
  markSessionRead,
  projectIndicator,
  indicatorLabel,
  sendMessage,
  stop,
  submitExtensionDialog,
  cancelExtensionDialog,
  dismissExtensionNotification,
  renameSession,
  selectModel,
  selectEffort,
} = useTau();

/**
 * An indicator without a state still reserves its slot, so it stays hidden
 * from assistive tech until it carries a meaning worth announcing.
 */
function indicatorAttrs(indicator: SessionIndicator) {
  const label = indicatorLabel(indicator);
  return label
    ? { class: indicator, title: label, role: "img", "aria-label": label }
    : { "aria-hidden": true };
}

const sessionIsEmpty = computed(
  () =>
    canDraft.value &&
    !sessionLoading.value &&
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
const filteredCommands = computed(() =>
  commandQuery.value === null
    ? []
    : filterCommands(commands.value, commandQuery.value),
);
const commandMenuActive = computed(
  () =>
    canDraft.value &&
    !commandMenuDismissed.value &&
    filteredCommands.value.length > 0,
);
const selectedCommand = computed(
  () => filteredCommands.value[commandSelectedIndex.value],
);
const commandMenuStyle = computed(() => ({
  maxHeight:
    commandMenuMaxHeight.value === undefined
      ? undefined
      : `${commandMenuMaxHeight.value}px`,
  top:
    commandMenuPlacement.value === "below"
      ? `${commandMenuOffset.value}px`
      : undefined,
}));
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
  setupProjectReordering();
  void watchWindowFocus();
  void watchMenuActions();
  document.addEventListener("pointerdown", handleDocumentPointerDown);
  document.addEventListener("contextmenu", handleDocumentContextMenu);
  document.addEventListener("keydown", handleDocumentKeydown);
  window.addEventListener("resize", updateCommandMenuLayout);
  window.addEventListener("resize", closeContextMenu);
});
onBeforeUnmount(() => {
  projectSortable?.destroy();
  dispose();
  clearTimeout(loadingIndicatorTimer);
  unlistenWindowFocus?.();
  unlistenNewSessionMenu?.();
  document.removeEventListener("pointerdown", handleDocumentPointerDown);
  document.removeEventListener("contextmenu", handleDocumentContextMenu);
  document.removeEventListener("keydown", handleDocumentKeydown);
  window.removeEventListener("resize", updateCommandMenuLayout);
  window.removeEventListener("resize", closeContextMenu);
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

watch(canRenameSession, (renamable) => {
  if (!renamable) cancelSessionRename();
});

watch(
  () => state.activeControllerKey,
  (controllerKey) => {
    cancelSessionRename();
    if (!controllerKey) return;
    void nextTick(() => {
      if (!state.remoteDialogOpen && !activeExtensionDialog.value) {
        composerInput.value?.focus();
      }
    });
  },
);

watch([commandQuery, commands], ([query]) => {
  commandSelectedIndex.value = 0;
  // A dismissed menu stays closed until the composer leaves the command it was
  // opened for, so Escape is not undone by the next keystroke.
  if (query === null) commandMenuDismissed.value = false;
});

watch([commandMenuActive, filteredCommands, status], () => {
  if (!commandMenuActive.value) return;
  void nextTick(updateCommandMenuLayout);
});

/**
 * Hydration is usually quicker than a spinner takes to read, and one that
 * arrives and leaves within a few frames reads as slower than none at all.
 */
watch(
  sessionLoading,
  (loading) => {
    clearTimeout(loadingIndicatorTimer);
    if (!loading) {
      loadingIndicatorVisible.value = false;
      return;
    }
    loadingIndicatorTimer = setTimeout(() => {
      loadingIndicatorVisible.value = true;
    }, LOADING_INDICATOR_DELAY_MS);
  },
  { immediate: true },
);

watch(sessionLoading, (loading) => {
  if (loading) return;
  void nextTick(() => {
    if (!state.remoteDialogOpen && !activeExtensionDialog.value) {
      composerInput.value?.focus();
    }
  });
});

function beginSessionRename() {
  if (!canRenameSession.value) return;
  sessionNameDraft.value = sessionTitle.value;
  renamingSession.value = true;
  void nextTick(() => {
    sessionTitleInput.value?.focus();
    sessionTitleInput.value?.select();
  });
}

function commitSessionRename() {
  // Escape and a lost runtime both close the field before its blur arrives,
  // and neither should apply the name that was left in it.
  if (!renamingSession.value) return;
  closeSessionRename();
  void renameSession(sessionNameDraft.value);
}

function cancelSessionRename() {
  if (!renamingSession.value) return;
  closeSessionRename();
}

/**
 * Committing on blur means focus has already moved on, so the composer is only
 * refocused when the field itself still holds focus, as it does after Enter,
 * Escape, or a runtime that stopped mid-rename.
 */
function closeSessionRename() {
  const focused = document.activeElement === sessionTitleInput.value;
  renamingSession.value = false;
  if (focused) void nextTick(() => composerInput.value?.focus());
}

function updateCommandMenuLayout() {
  const menu = commandMenu.value;
  const anchor = composerInput.value;
  const container = composer.value;
  if (!menu || !anchor || !container) return;

  const anchorStyle = getComputedStyle(anchor);
  const anchorRect = anchor.getBoundingClientRect();
  const layout = commandMenuLayout({
    contentHeight: menu.scrollHeight + menu.offsetHeight - menu.clientHeight,
    composerTop: container.getBoundingClientRect().top,
    textTop: anchorRect.top + (Number.parseFloat(anchorStyle.paddingTop) || 0),
    textLineHeight: Number.parseFloat(anchorStyle.lineHeight) || 0,
    topBoundary: sessionHeader.value?.getBoundingClientRect().bottom ?? 0,
    bottomBoundary: window.innerHeight,
  });

  commandMenuPlacement.value = layout.placement;
  commandMenuMaxHeight.value = layout.maxHeight;
  commandMenuOffset.value = layout.offset;
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
  transcriptView.value?.scrollToEnd();
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

function openSessionMenu(
  project: ProjectSummary,
  session: SessionSummary,
  event: MouseEvent,
) {
  const unread = isSessionUnread(project, session);
  const items: ContextMenuItem[] = [
    {
      label: unread ? "Mark as Read" : "Mark as Unread",
      run: () =>
        unread
          ? markSessionRead(project, session)
          : markSessionUnread(project, session),
    },
  ];
  if (canArchiveSession(project, session)) {
    items.push({
      label: "Archive Session",
      run: () => void archiveSession(project, session),
    });
  }
  openContextMenu(event, items);
}

function openTextFieldMenu(event: MouseEvent, field: TextField) {
  // The menu takes focus, so the range the pointer landed on is captured now
  // and handed back to the field when an item runs.
  const start = field.selectionStart ?? 0;
  const end = field.selectionEnd ?? 0;
  const selected = start !== end;
  const editable = !field.readOnly && !field.disabled;
  openContextMenu(event, [
    {
      label: "Cut",
      disabled: !selected || !editable,
      run: () => void copyField(field, start, end, editable),
    },
    {
      label: "Copy",
      disabled: !selected,
      run: () => void copyField(field, start, end, false),
    },
    {
      label: "Paste",
      disabled: !editable,
      run: () => void pasteField(field, start, end),
    },
  ]);
}

async function copyField(
  field: TextField,
  start: number,
  end: number,
  cut: boolean,
) {
  const text = field.value.slice(start, end);
  if (!text) return;
  try {
    await writeText(text);
  } catch {
    return;
  }
  if (cut) replaceFieldRange(field, start, end, "");
}

async function pasteField(field: TextField, start: number, end: number) {
  let text = "";
  try {
    text = await readText();
  } catch {
    return;
  }
  if (text) replaceFieldRange(field, start, end, text);
}

/**
 * Fields are bound with v-model, so the edit is announced with an input event
 * rather than written to the reactive state each field happens to use.
 */
function replaceFieldRange(
  field: TextField,
  start: number,
  end: number,
  text: string,
) {
  field.value = field.value.slice(0, start) + text + field.value.slice(end);
  const caret = start + text.length;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  field.focus();
  field.setSelectionRange(caret, caret);
}

function openContextMenu(event: MouseEvent, items: ContextMenuItem[]) {
  projectMenuOpen.value = false;
  contextMenuState.value = { items, x: event.clientX, y: event.clientY };
  void nextTick(() => {
    keepContextMenuOnScreen();
    contextMenu.value
      ?.querySelector<HTMLButtonElement>("button:not(:disabled)")
      ?.focus();
  });
}

/**
 * The menu opens at the pointer, which near the bottom or right edge would
 * otherwise place part of it outside the window.
 */
function keepContextMenuOnScreen() {
  const menu = contextMenuState.value;
  const element = contextMenu.value;
  if (!menu || !element) return;
  const { width, height } = element.getBoundingClientRect();
  const maxX = window.innerWidth - width - CONTEXT_MENU_MARGIN;
  const maxY = window.innerHeight - height - CONTEXT_MENU_MARGIN;
  menu.x = Math.max(CONTEXT_MENU_MARGIN, Math.min(menu.x, maxX));
  menu.y = Math.max(CONTEXT_MENU_MARGIN, Math.min(menu.y, maxY));
}

function closeContextMenu() {
  contextMenuState.value = undefined;
}

function runContextMenuItem(item: ContextMenuItem) {
  closeContextMenu();
  item.run();
}

function setupProjectReordering() {
  if (!projectList.value) return;
  projectSortable = Sortable.create(projectList.value, {
    animation: 180,
    handle: ".project-drag-handle",
    draggable: ".project-group",
    ghostClass: "project-sortable-ghost",
    chosenClass: "project-sortable-chosen",
    dragClass: "project-sortable-drag",
    forceFallback: true,
    fallbackOnBody: true,
    fallbackTolerance: 3,
    onEnd: finishProjectReordering,
  });
}

function finishProjectReordering(event: SortableEvent) {
  if (event.oldIndex === undefined || event.newIndex === undefined) return;
  void reorderProjects(event.oldIndex, event.newIndex);
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
  if (
    contextMenuState.value &&
    (!(target instanceof Node) || !contextMenu.value?.contains(target))
  ) {
    closeContextMenu();
  }
}

/**
 * Window focus is not document focus: the document inside a webview keeps focus
 * while the app sits in the background, so the shell has to report it instead.
 */
async function watchWindowFocus() {
  try {
    unlistenWindowFocus = await getCurrentWindow().onFocusChanged(
      ({ payload }) => {
        windowFocused.value = payload;
      },
    );
  } catch {
    // Running in a plain browser, which has no window to follow.
  }
}

async function watchMenuActions() {
  try {
    unlistenNewSessionMenu = await listen(NEW_SESSION_EVENT, handleNewSession);
  } catch {
    // Running in a plain browser, which has no menu bar.
  }
}

/**
 * The webview's own menu offers reloads and page navigation, which a desktop
 * app has no use for, so every menu in Tau is its own. Text fields still need
 * the editing actions the native menu would have carried.
 */
function handleDocumentContextMenu(event: MouseEvent) {
  event.preventDefault();
  const target = event.target;
  const field =
    target instanceof Element ? target.closest(TEXT_FIELD_SELECTOR) : null;
  if (
    field instanceof HTMLInputElement ||
    field instanceof HTMLTextAreaElement
  ) {
    openTextFieldMenu(event, field);
  }
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.defaultPrevented) return;
  if (event.key === "Escape") {
    handleEscape(event);
    return;
  }
  suppressSystemBeep(event);
}

/** Escape closes the innermost surface that is open, innermost first. */
function handleEscape(event: KeyboardEvent) {
  if (contextMenuState.value) {
    event.preventDefault();
    closeContextMenu();
  } else if (activeExtensionDialog.value) {
    event.preventDefault();
    void cancelExtensionDialog();
  } else if (state.remoteDialogOpen) closeRemoteProjectDialog();
  else if (commandMenuActive.value) {
    event.preventDefault();
    commandMenuDismissed.value = true;
  } else projectMenuOpen.value = false;
}

/**
 * WKWebView rings the system bell for a keystroke nothing can take, which is
 * every letter typed while a button holds focus. Space is left alone because it
 * activates the focused control.
 */
function suppressSystemBeep(event: KeyboardEvent) {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  if (event.key.length !== 1 || event.key === " ") return;
  if (isEditableTarget(event.target)) return;
  event.preventDefault();
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element && Boolean(target.closest(EDITABLE_SELECTOR))
  );
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
  if (
    event.button !== 0 ||
    event.detail > 1 ||
    isTitlebarControl(event.target)
  ) {
    return;
  }
  void getCurrentWindow()
    .startDragging()
    .catch(() => undefined);
}

function handleTitlebarDoubleClick(event: MouseEvent) {
  if (event.button !== 0 || isTitlebarControl(event.target)) return;
  event.preventDefault();
  void getCurrentWindow()
    .toggleMaximize()
    .catch(() => undefined);
}

function isTitlebarControl(target: EventTarget | null): boolean {
  return (
    !(target instanceof Element) ||
    Boolean(target.closest("button, a, input, select, textarea"))
  );
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
    :class="{
      'resizing-sidebar': resizingSidebar,
      'window-inactive': !windowFocused,
    }"
    :style="{ '--sidebar-width': `${sidebarWidth}px` }"
  >
    <aside ref="sidebar" class="sidebar">
      <header
        class="sidebar-titlebar"
        @mousedown="handleTitlebarMouseDown"
        @dblclick="handleTitlebarDoubleClick"
      ></header>

      <div
        ref="projectList"
        class="project-list"
        @scroll.passive="closeContextMenu"
      >
        <div
          v-for="project in state.workspace?.projects"
          :key="project.path"
          class="project-group"
          :data-id="project.path"
        >
          <div
            class="project-row"
            :class="{ selected: project.path === state.activeProjectPath }"
          >
            <span
              class="project-drag-handle"
              title="Drag to reorder"
              aria-hidden="true"
            >
              <UiIcon name="grip" />
            </span>
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
              <span
                v-if="projectIndicator(project)"
                class="session-indicator"
                v-bind="indicatorAttrs(projectIndicator(project))"
              ></span>
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
              @contextmenu.prevent="openSessionMenu(project, session, $event)"
            >
              <button
                class="session-select"
                type="button"
                @click="selectSession(project, session)"
              >
                <span
                  class="session-indicator"
                  v-bind="indicatorAttrs(sessionIndicator(project, session))"
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
        </div>

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

    <main
      class="session-pane"
      :class="{
        'empty-session': sessionIsEmpty,
        'loading-session': sessionLoading,
      }"
    >
      <header
        ref="sessionHeader"
        class="session-header"
        @mousedown="handleTitlebarMouseDown"
        @dblclick="handleTitlebarDoubleClick"
      >
        <div class="session-heading">
          <input
            v-if="renamingSession"
            ref="sessionTitleInput"
            v-model="sessionNameDraft"
            class="session-name-input"
            type="text"
            maxlength="240"
            spellcheck="false"
            aria-label="Session name"
            @blur="commitSessionRename"
            @keydown.enter.prevent="commitSessionRename"
            @keydown.escape.prevent="cancelSessionRename"
          />
          <h1 v-else>
            <button
              v-if="canRenameSession"
              class="session-name"
              type="button"
              title="Rename session"
              @click="beginSessionRename"
            >
              {{ sessionTitle }}
            </button>
            <span v-else class="session-name">{{ sessionTitle }}</span>
          </h1>
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

      <div v-if="sessionLoading" class="session-loading">
        <template v-if="loadingIndicatorVisible">
          <PiSpinner label="Loading session" />
          <span>Loading</span>
        </template>
      </div>

      <TranscriptView
        v-else-if="!sessionIsEmpty"
        :key="state.activeControllerKey"
        ref="transcriptView"
        :messages="messages"
        :show-working-indicator="showWorkingIndicator"
        :working-label="stopping ? 'Pi is stopping' : 'Pi is working'"
      />

      <footer v-if="!sessionLoading" class="composer-area">
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
            <MarkdownText
              class="extension-dialog-title"
              inline
              :source="activeExtensionDialog.title"
              :base-path="activeExtensionDialog.workingDirectory"
            />
          </header>

          <MarkdownText
            v-if="activeExtensionDialog.message"
            class="extension-dialog-message"
            :source="activeExtensionDialog.message"
            :base-path="activeExtensionDialog.workingDirectory"
          />

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
        <div
          v-else
          ref="composer"
          class="composer"
          :class="{ disabled: !canDraft }"
        >
          <div
            v-if="commandMenuActive"
            id="command-menu"
            ref="commandMenu"
            class="command-menu"
            :class="commandMenuPlacement"
            :style="commandMenuStyle"
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
      v-if="contextMenuState"
      ref="contextMenu"
      class="context-menu"
      role="menu"
      :style="{
        left: `${contextMenuState.x}px`,
        top: `${contextMenuState.y}px`,
      }"
    >
      <button
        v-for="item in contextMenuState.items"
        :key="item.label"
        type="button"
        role="menuitem"
        :disabled="item.disabled"
        @click="runContextMenuItem(item)"
      >
        {{ item.label }}
      </button>
    </div>

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
          <MarkdownText
            class="extension-notification-message"
            :source="notification.message"
            :base-path="notification.workingDirectory"
          />
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
