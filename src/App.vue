<template>
  <div
    class="app-shell"
    :class="{
      'resizing-sidebar': resizingSidebar,
      'window-inactive': !windowFocused,
    }"
    :style="{ '--sidebar-width': `${sidebarWidth}px` }"
  >
    <ProjectSidebar
      v-model:sidebar-width="sidebarWidth"
      v-model:resizing="resizingSidebar"
    />

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
        :base-path="transcriptBasePath"
        :session-key="state.activeControllerKey"
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
import ProjectSidebar from './components/ProjectSidebar.vue';
import RemoteDialog from './components/RemoteDialog.vue';
import SessionHeader from './components/SessionHeader.vue';
import TranscriptView from './components/TranscriptView.vue';
import UiSpinner from './components/ui/UiSpinner.vue';
import useTau from './composables/useTau';
import { loadSidebarWidth } from './lib/sidebar-width';

const NEW_SESSION_EVENT = 'tau://new-session';
/** How long a session may hydrate before it is worth reporting as loading. */
const LOADING_INDICATOR_DELAY_MS = 200;
const EDITABLE_SELECTOR = 'input, textarea, select';

const transcriptView = ref<InstanceType<typeof TranscriptView>>();
const sessionHeader = ref<InstanceType<typeof SessionHeader>>();
const composerBar = ref<InstanceType<typeof ComposerBar>>();
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
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
  sessionLoading,
  initialize,
  dispose,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  newSession,
  sendMessage,
  submitExtensionDialog,
  cancelExtensionDialog,
} = useTau();

/**
 * An indicator without a state still reserves its slot, so it stays hidden
 * from assistive tech until it carries a meaning worth announcing. UiStatusDot
 * renders that itself from the label.
 */

const sessionIsEmpty = computed(
  () => canDraft.value && !sessionLoading.value && messages.value.length === 0,
);
const transcriptBasePath = computed(() =>
  activeProject.value && !activeProject.value.connectionString
    ? activeProject.value.workingDirectory
    : undefined,
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
  void watchWindowFocus();
  void watchMenuActions();
  document.addEventListener('contextmenu', handleDocumentContextMenu);
  document.addEventListener('keydown', handleDocumentKeydown);
});
onBeforeUnmount(() => {
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
</script>

<style scoped>
.app-shell {
  --sidebar-width: 260px;

  display: grid;
  grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

/* Native chrome dims its selection while its window sits in the background. */
.app-shell.window-inactive {
  --selected: var(--selected-inactive);
}

.app-shell.resizing-sidebar,
.app-shell.resizing-sidebar * {
  cursor: col-resize;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: none;
  user-select: none;
}

.app-shell.resizing-sidebar :deep(.sidebar-resize-handle)::after {
  background: var(--muted);
}

.session-pane {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  min-width: 0;
  min-height: 0;
}

.session-pane.empty-session,
.session-pane.loading-session {
  grid-template-rows: auto minmax(0, 1fr);
}

.session-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding-bottom: 24px;
  color: var(--muted);
  font-size: 12px;
}

.composer-area {
  padding: 6px 8px 8px 6px;
  border-top: 1px solid var(--border);
  background: var(--canvas);
}

.empty-session .composer-area,
.empty-session :deep(.composer) {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.empty-session .composer-area {
  border-top: 0;
}

.empty-session .composer-area:has(.extension-composer) {
  justify-content: flex-end;
}

.empty-session :deep(.composer textarea) {
  flex: 1;
  max-height: none;
  padding-top: 16px;
}
</style>
