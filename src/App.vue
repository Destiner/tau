<template>
  <div
    class="app-shell"
    :class="{
      'resizing-sidebar': resizingSidebar,
      'window-inactive': !windowFocused,
    }"
    :style="{ '--sidebar-width': `${sidebarWidth}px` }"
  >
    <aside
      ref="sidebar"
      class="sidebar"
    >
      <header
        class="sidebar-titlebar"
        @mousedown="handleTitlebarMouseDown"
        @dblclick="handleTitlebarDoubleClick"
      ></header>

      <div
        ref="projectList"
        class="project-list"
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
              @click="() => toggleProject(project)"
            >
              <span>{{ project.name }}</span>
              <UiIcon
                name="chevron"
                :class="{ expanded: !project.collapsed }"
              />
              <UiStatusDot
                v-if="projectIndicator(project)"
                :tone="projectIndicator(project) || undefined"
                :label="indicatorLabel(projectIndicator(project))"
              />
            </button>
            <UiIconButton
              class="row-action"
              size="md"
              variant="reveal"
              :label="`New session in ${project.name}`"
              title="New session"
              @click="() => newSession(project)"
            >
              <UiIcon name="plus" />
            </UiIconButton>
            <UiIconButton
              class="row-action"
              size="md"
              variant="reveal"
              tone="danger"
              :label="`Remove ${project.name}`"
              title="Remove project"
              @click="() => removeProject(project)"
            >
              <UiIcon name="trash" />
            </UiIconButton>
          </div>

          <div
            v-if="!project.collapsed"
            class="session-list"
          >
            <UiContextMenu
              v-for="session in projectSessions(project)"
              :key="session.id"
              :items="() => sessionMenuItems(project, session)"
            >
              <div
                class="session-row"
                :class="{
                  selected: isSessionSelected(project, session),
                  archivable: canArchiveSession(project, session),
                }"
              >
                <button
                  class="session-select"
                  type="button"
                  @click="() => selectSession(project, session)"
                >
                  <UiStatusDot
                    :tone="sessionIndicator(project, session) || undefined"
                    :label="indicatorLabel(sessionIndicator(project, session))"
                  />
                  <span class="session-copy">
                    <span class="session-title">{{ session.title }}</span>
                    <span class="session-time">{{
                      sessionLastActive(project, session)
                    }}</span>
                  </span>
                </button>
                <UiIconButton
                  v-if="canArchiveSession(project, session)"
                  class="session-archive"
                  size="md"
                  variant="reveal"
                  :label="`Archive ${session.title}`"
                  title="Archive session"
                  @click="() => archiveSession(project, session)"
                >
                  <UiIcon name="archive" />
                </UiIconButton>
              </div>
            </UiContextMenu>
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
        <div class="project-menu-wrap">
          <UiMenu
            v-model:open="projectMenuOpen"
            :items="projectMenuItems"
            :min-width="176"
          >
            <template #trigger>
              <button
                class="icon-button"
                type="button"
                title="Open project"
                aria-label="Open project"
              >
                <UiIcon name="folder" />
              </button>
            </template>
          </UiMenu>
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
      <SessionHeader
        ref="sessionHeader"
        @mousedown="handleTitlebarMouseDown"
        @dblclick="handleTitlebarDoubleClick"
        @composer-focus="focusComposer"
      />

      <div
        v-if="sessionLoading"
        class="session-loading"
      >
        <template v-if="loadingIndicatorVisible">
          <UiSpinner label="Loading session" />
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

      <footer
        v-if="!sessionLoading"
        class="composer-area"
      >
        <ExtensionDialog
          v-if="activeExtensionDialog"
          v-model:draft="activeExtensionDialog.draft"
          :method="activeExtensionDialog.method"
          :title="activeExtensionDialog.title"
          :message="activeExtensionDialog.message"
          :options="activeExtensionDialog.options"
          :placeholder="activeExtensionDialog.placeholder"
          :project-name="activeExtensionDialog.projectName"
          :session-name="activeExtensionDialog.sessionName"
          :working-directory="activeExtensionDialog.workingDirectory"
          @submit="handleExtensionSubmit"
          @cancel="cancelExtensionDialog"
        />
        <ComposerBar
          v-else
          :header-element="() => sessionHeader?.header"
          @send="handleComposerSend"
        />
      </footer>
    </main>

    <NotificationStack
      :notifications="extensionNotifications"
      @dismiss="dismissExtensionNotification"
    />

    <RemoteDialog
      v-model:open="state.remoteDialogOpen"
      v-model:connection-string="state.remoteConnectionString"
      v-model:directory-filter="state.remoteDirectoryFilter"
      v-model:selected-index="state.remoteDirectorySelectedIndex"
      :step="state.remoteDialogStep"
      :mode="state.remoteDialogMode"
      :connection-error="state.remoteConnectionError"
      :connecting="state.remoteConnecting"
      :directory-options="remoteDirectoryOptions"
      @submit-connection="submitRemoteConnection"
      @choose-directory="handleChooseDirectory"
    />
  </div>
</template>

<script setup lang="ts">
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import Sortable, { type SortableEvent } from 'sortablejs';
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import ComposerBar from './components/ComposerBar.vue';
import ExtensionDialog from './components/ExtensionDialog.vue';
import NotificationStack from './components/NotificationStack.vue';
import RemoteDialog from './components/RemoteDialog.vue';
import SessionHeader from './components/SessionHeader.vue';
import TranscriptView from './components/TranscriptView.vue';
import UiContextMenu from './components/ui/UiContextMenu.vue';
import UiIcon from './components/ui/UiIcon.vue';
import UiIconButton from './components/ui/UiIconButton.vue';
import UiMenu from './components/ui/UiMenu.vue';
import type { UiMenuItem } from './components/ui/UiMenu.vue';
import UiSpinner from './components/ui/UiSpinner.vue';
import UiStatusDot from './components/ui/UiStatusDot.vue';
import type { ProjectSummary, SessionSummary } from './composables/state';
import useTau from './composables/useTau';

const SIDEBAR_WIDTH_STORAGE_KEY = 'tau.sidebar-width';
const NEW_SESSION_EVENT = 'tau://new-session';
/** How long a session may hydrate before it is worth reporting as loading. */
const LOADING_INDICATOR_DELAY_MS = 200;
const EDITABLE_SELECTOR = 'input, textarea, select';
const DEFAULT_SIDEBAR_WIDTH = 260;
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 480;

const transcriptView = ref<InstanceType<typeof TranscriptView>>();
const sessionHeader = ref<InstanceType<typeof SessionHeader>>();
const composerBar = ref<InstanceType<typeof ComposerBar>>();
const projectList = ref<HTMLElement>();
const sidebar = ref<HTMLElement>();
const projectMenuOpen = ref(false);
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
let projectSortable: Sortable | undefined;
let unlistenWindowFocus: UnlistenFn | undefined;
let unlistenNewSessionMenu: UnlistenFn | undefined;
let loadingIndicatorTimer: ReturnType<typeof setTimeout> | undefined;
const {
  state,
  activeProject,
  messages,
  canDraft,
  streaming,
  stopping,
  activeExtensionDialog,
  extensionNotifications,
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
  submitExtensionDialog,
  cancelExtensionDialog,
  dismissExtensionNotification,
} = useTau();

/**
 * An indicator without a state still reserves its slot, so it stays hidden
 * from assistive tech until it carries a meaning worth announcing. UiStatusDot
 * renders that itself from the label.
 */

const sessionIsEmpty = computed(
  () =>
    canDraft.value &&
    !sessionLoading.value &&
    !messages.value.some(
      (message) => message.kind === 'user' || message.kind === 'assistant',
    ),
);
const showWorkingIndicator = computed(() => {
  if (stopping.value) return true;
  if (!streaming.value) return false;
  return messages.value[messages.value.length - 1]?.kind !== 'assistant';
});
const remoteDirectoryOptions = computed(() => {
  if (!state.remoteWorkingDirectory) return [];
  const options = [] as Array<{
    name: string;
    path: string;
    kind: 'back' | 'select' | 'forward';
  }>;
  const previousDirectory =
    state.remoteWorkingDirectory === state.remoteDirectoryRoot
      ? undefined
      : state.remoteDirectoryHistory[state.remoteDirectoryHistory.length - 1];
  if (previousDirectory) {
    options.push({
      name: 'Go Back',
      path: previousDirectory,
      kind: 'back',
    });
  }
  options.push({
    name: `Select ${state.remoteWorkingDirectory}`,
    path: state.remoteWorkingDirectory,
    kind: 'select',
  });
  options.push(
    ...state.remoteDirectories.map((directory) => ({
      ...directory,
      kind: 'forward' as const,
    })),
  );

  const filter = state.remoteDirectoryFilter.trim().toLocaleLowerCase();
  return options.filter((option) => {
    const name = option.name.toLocaleLowerCase();
    if (option.kind === 'forward' && option.name.startsWith('.')) {
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
  document.addEventListener('contextmenu', handleDocumentContextMenu);
  document.addEventListener('keydown', handleDocumentKeydown);
});
onBeforeUnmount(() => {
  projectSortable?.destroy();
  dispose();
  clearTimeout(loadingIndicatorTimer);
  unlistenWindowFocus?.();
  unlistenNewSessionMenu?.();
  document.removeEventListener('contextmenu', handleDocumentContextMenu);
  document.removeEventListener('keydown', handleDocumentKeydown);
});

watch(activeExtensionDialog, (dialog) => {
  void nextTick(() => {
    if (!dialog) composerBar.value?.focus();
  });
});

/**
 * Closing the remote dialog anywhere — Escape, the scrim, a finished
 * connection — runs the same cleanup the explicit close used to.
 */
watch(
  () => state.remoteDialogOpen,
  (open) => {
    if (!open) closeRemoteProjectDialog();
  },
);

watch(
  () => state.activeControllerKey,
  (controllerKey) => {
    sessionHeader.value?.cancelRename();
    if (!controllerKey) return;
    void nextTick(() => {
      if (!state.remoteDialogOpen && !activeExtensionDialog.value) {
        composerBar.value?.focus();
      }
    });
  },
);

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
      composerBar.value?.focus();
    }
  });
});

function focusComposer(): void {
  composerBar.value?.focus();
}

/** A send starts with the transcript pinned to its end. */
function handleComposerSend(): void {
  transcriptView.value?.scrollToEnd();
  void sendMessage();
}

function handleExtensionSubmit(value: string | boolean): void {
  void submitExtensionDialog(value);
}

function handleChooseDirectory(
  path: string,
  kind: 'back' | 'select' | 'forward',
): void {
  void chooseRemoteDirectory(path, kind);
}

function handleNewSession(): void {
  if (activeProject.value) void newSession(activeProject.value);
}

/** The project menu's two ways to add a project. */
const projectMenuItems: UiMenuItem[] = [
  {
    label: 'Open Local Project',
    run: () => void handleLocalProject(),
  },
  {
    label: 'Open Remote Project',
    run: () => void handleRemoteProject(),
  },
];

function sessionMenuItems(
  project: ProjectSummary,
  session: SessionSummary,
): UiMenuItem[] {
  const unread = isSessionUnread(project, session);
  const items: UiMenuItem[] = [
    {
      label: unread ? 'Mark as Read' : 'Mark as Unread',
      run: () =>
        unread
          ? markSessionRead(project, session)
          : markSessionUnread(project, session),
    },
  ];
  if (canArchiveSession(project, session)) {
    items.push({
      label: 'Archive Session',
      run: () => void archiveSession(project, session),
    });
  }
  return items;
}

function setupProjectReordering(): void {
  if (!projectList.value) return;
  projectSortable = Sortable.create(projectList.value, {
    animation: 180,
    handle: '.project-drag-handle',
    draggable: '.project-group',
    ghostClass: 'project-sortable-ghost',
    chosenClass: 'project-sortable-chosen',
    dragClass: 'project-sortable-drag',
    forceFallback: true,
    fallbackOnBody: true,
    fallbackTolerance: 3,
    onEnd: finishProjectReordering,
  });
}

function finishProjectReordering(event: SortableEvent): void {
  if (event.oldIndex === undefined || event.newIndex === undefined) return;
  void reorderProjects(event.oldIndex, event.newIndex);
}

function startSidebarResize(event: PointerEvent): void {
  if (event.button !== 0) return;
  const handle = event.currentTarget;
  if (!(handle instanceof HTMLElement)) return;

  event.preventDefault();
  resizingSidebar.value = true;
  handle.setPointerCapture(event.pointerId);
  handle.focus({ preventScroll: true });
  updateSidebarWidth(event.clientX);
}

function handleSidebarResize(event: PointerEvent): void {
  if (resizingSidebar.value) updateSidebarWidth(event.clientX);
}

function finishSidebarResize(event: PointerEvent): void {
  if (!resizingSidebar.value) return;
  updateSidebarWidth(event.clientX);
  stopSidebarResize();
}

function stopSidebarResize(): void {
  if (!resizingSidebar.value) return;
  resizingSidebar.value = false;
  persistSidebarWidth();
}

function handleSidebarResizeKeydown(event: KeyboardEvent): void {
  const step = event.shiftKey ? 40 : 10;
  let nextWidth: number;

  switch (event.key) {
    case 'ArrowLeft':
      nextWidth = sidebarWidth.value - step;
      break;
    case 'ArrowRight':
      nextWidth = sidebarWidth.value + step;
      break;
    case 'Home':
      nextWidth = MIN_SIDEBAR_WIDTH;
      break;
    case 'End':
      nextWidth = MAX_SIDEBAR_WIDTH;
      break;
    default:
      return;
  }

  event.preventDefault();
  sidebarWidth.value = clampSidebarWidth(nextWidth);
  persistSidebarWidth();
}

function updateSidebarWidth(pointerX: number): void {
  const left = sidebar.value?.getBoundingClientRect().left ?? 0;
  sidebarWidth.value = clampSidebarWidth(pointerX - left);
}

function persistSidebarWidth(): void {
  try {
    localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(sidebarWidth.value));
  } catch {
    return;
  }
}

/**
 * Window focus is not document focus: the document inside a webview keeps focus
 * while the app sits in the background, so the shell has to report it instead.
 */
async function watchWindowFocus(): Promise<void> {
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

async function watchMenuActions(): Promise<void> {
  try {
    unlistenNewSessionMenu = await listen(NEW_SESSION_EVENT, handleNewSession);
  } catch {
    // Running in a plain browser, which has no menu bar.
  }
}

/**
 * The webview's own menu offers reloads and page navigation, which a desktop
 * app has no use for, so every menu in Tau is its own. The menus themselves
 * are reka's; this only keeps the webview's native one from ever appearing.
 */
function handleDocumentContextMenu(event: MouseEvent): void {
  event.preventDefault();
}

function handleDocumentKeydown(event: KeyboardEvent): void {
  if (event.defaultPrevented) return;
  if (event.key === 'Escape') {
    handleEscape(event);
    return;
  }
  suppressSystemBeep(event);
}

/** Escape closes the innermost surface that is open, innermost first. */
function handleEscape(event: KeyboardEvent): void {
  if (activeExtensionDialog.value) {
    event.preventDefault();
    void cancelExtensionDialog();
  }
}

/**
 * WKWebView rings the system bell for a keystroke nothing can take, which is
 * every letter typed while a button holds focus. Space is left alone because it
 * activates the focused control.
 */
function suppressSystemBeep(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  if (event.key.length !== 1 || event.key === ' ') return;
  if (isEditableTarget(event.target)) return;
  event.preventDefault();
}

function isEditableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element && Boolean(target.closest(EDITABLE_SELECTOR))
  );
}

function handleLocalProject(): void {
  void addLocalProject();
}

function handleRemoteProject(): void {
  openRemoteProjectDialog();
}

function handleTitlebarMouseDown(event: MouseEvent): void {
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

function handleTitlebarDoubleClick(event: MouseEvent): void {
  if (event.button !== 0 || isTitlebarControl(event.target)) return;
  event.preventDefault();
  void getCurrentWindow()
    .toggleMaximize()
    .catch(() => undefined);
}

function isTitlebarControl(target: EventTarget | null): boolean {
  return (
    !(target instanceof Element) ||
    Boolean(target.closest('button, a, input, select, textarea'))
  );
}

function loadSidebarWidth(): number {
  try {
    const storedWidth = Number.parseFloat(
      localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? '',
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
