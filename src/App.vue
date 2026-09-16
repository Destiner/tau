<template>
  <div
    class="app-shell"
    :class="{
      'empty-workspace': workspaceShellVisible,
      'resizing-sidebar': resizingSidebar,
      'window-inactive': !windowFocused,
    }"
    :style="{ '--sidebar-width': `${sidebarWidth}px` }"
  >
    <main
      v-if="workspaceShellVisible"
      class="first-run"
    >
      <header
        class="first-run-titlebar"
        @mousedown="handleTitlebarMouseDown"
        @dblclick="handleTitlebarDoubleClick"
      ></header>
      <section class="first-run-content">
        <template v-if="workspaceIsEmpty">
          <p
            class="first-run-version"
            :aria-label="`Tau version ${appVersion}`"
          >
            <span class="first-run-name">tau</span>
            <span class="first-run-version-number">{{ appVersion }}</span>
          </p>
          <div class="first-run-actions">
            <UiButton
              ref="firstRunLocalProjectButton"
              variant="ghost"
              size="md"
              @click="addLocalProject"
            >
              Open Local Project
            </UiButton>
            <UiButton
              ref="firstRunRemoteProjectButton"
              variant="ghost"
              size="md"
              @click="openRemoteProjectDialog"
            >
              Open Remote Project
            </UiButton>
          </div>
        </template>
        <p
          v-if="state.workspaceStatus"
          class="first-run-error"
          role="alert"
        >
          {{ state.workspaceStatus }}
        </p>
      </section>
    </main>

    <template v-else>
      <ProjectSidebar
        ref="projectSidebar"
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
          :compacting="compacting"
          :show-working-indicator="showWorkingIndicator"
          :working-label="stopping ? 'Stopping' : 'Working'"
          :base-path="transcriptBasePath"
          :copy-paths="Boolean(activeProject?.connectionString)"
          :session-key="state.activeControllerKey"
          :prompt="activeExtensionDialog"
          :prompt-disabled="projectActionsDisabled"
          @prompt-submit="handleExtensionSubmit"
          @prompt-cancel="cancelExtensionDialog"
          @prompt-draft="updateExtensionDraft"
          @load-history="loadEarlierHistory"
        />

        <!--
        A prompt takes the composer's place rather than sitting above it: the
        session is waiting on an answer, so there is nothing to send.
      -->
        <footer
          v-if="!sessionLoading && !activeExtensionDialog"
          class="composer-area"
        >
          <ComposerBar
            ref="composerBar"
            :header-element="() => sessionHeader?.header"
            @send="handleComposerSend"
          />
        </footer>
      </main>
    </template>

    <QuitConfirmation
      :open="quitRequest !== null"
      :session-count="quitSessionCount"
      :busy="quitBusy"
      :error="quitError"
      @cancel="cancelQuit"
      @confirm="confirmQuit"
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
      :return-focus="remoteDialogReturnFocus"
      @submit-connection="submitRemoteConnection"
      @choose-directory="handleChooseDirectory"
    />
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
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
import ProjectSidebar from './components/ProjectSidebar.vue';
import QuitConfirmation from './components/QuitConfirmation.vue';
import RemoteDialog from './components/RemoteDialog.vue';
import SessionHeader from './components/SessionHeader.vue';
import TranscriptView from './components/TranscriptView.vue';
import UiButton from './components/ui/UiButton.vue';
import UiSpinner from './components/ui/UiSpinner.vue';
import useTau from './composables/useTau';
import { createAdminCodeMatcher } from './lib/admin-code';
import { toggleAdminMode } from './lib/admin-mode';
import appVersion from './lib/app-version';
import { loadSidebarWidth } from './lib/sidebar-width';
import { invokeTraced } from './lib/telemetry';

const NEW_SESSION_EVENT = 'tau://new-session';
const QUIT_REQUEST_EVENT = 'tau://quit-requested';
/** How long a session may hydrate before it is worth reporting as loading. */
const LOADING_INDICATOR_DELAY_MS = 200;
const EDITABLE_SELECTOR = 'input, textarea, select';

interface QuitRequest {
  requestId: number;
}

const transcriptView = ref<InstanceType<typeof TranscriptView>>();
const projectSidebar = ref<InstanceType<typeof ProjectSidebar>>();
const sessionHeader = ref<InstanceType<typeof SessionHeader>>();
const composerBar = ref<InstanceType<typeof ComposerBar>>();
const firstRunLocalProjectButton = ref<InstanceType<typeof UiButton>>();
const firstRunRemoteProjectButton = ref<InstanceType<typeof UiButton>>();
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
const quitRequest = ref<QuitRequest | null>(null);
const quitSessionCount = ref(0);
const quitBusy = ref(false);
const quitError = ref('');
const adminCode = createAdminCodeMatcher(isEditableTarget);
let unlistenWindowFocus: UnlistenFn | undefined;
let unlistenNewSessionMenu: UnlistenFn | undefined;
let unlistenQuitRequest: UnlistenFn | undefined;
let loadingIndicatorTimer: ReturnType<typeof setTimeout> | undefined;
const {
  state,
  activeProject,
  projectActionsDisabled,
  messages,
  canDraft,
  streaming,
  compacting,
  stopping,
  promptSubmitting,
  inProgressSessionCount,
  activeExtensionDialog,
  sessionLoading,
  initialize,
  addLocalProject,
  openRemoteProjectDialog,
  dispose,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  newSession,
  sendMessage,
  loadEarlierHistory,
  submitExtensionDialog,
  cancelExtensionDialog,
} = useTau();

/**
 * An indicator without a state still reserves its slot, so it stays hidden
 * from assistive tech until it carries a meaning worth announcing. UiStatusDot
 * renders that itself from the label.
 */

const workspaceIsEmpty = computed(
  () => state.workspace !== null && state.workspace.projects.length === 0,
);
const workspaceShellVisible = computed(
  () => state.workspace === null || workspaceIsEmpty.value,
);
const sessionIsEmpty = computed(
  () =>
    canDraft.value &&
    !sessionLoading.value &&
    messages.value.length === 0 &&
    !compacting.value &&
    !activeExtensionDialog.value,
);
const transcriptBasePath = computed(
  () => activeProject.value?.workingDirectory,
);
const showWorkingIndicator = computed(() => {
  if (stopping.value) return true;
  if (!streaming.value || compacting.value) return false;
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
  unlistenQuitRequest?.();
  document.removeEventListener('contextmenu', handleDocumentContextMenu);
  document.removeEventListener('keydown', handleDocumentKeydown);
});

watch(workspaceIsEmpty, (empty) => {
  if (!empty) return;
  void nextTick(() => firstRunLocalProjectButton.value?.button?.focus());
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

watch(promptSubmitting, () => {
  void nextTick(() => {
    if (!state.remoteDialogOpen && !activeExtensionDialog.value) {
      composerBar.value?.focus();
    }
  });
});

function focusComposer(): void {
  composerBar.value?.focus();
}

function remoteDialogReturnFocus(): HTMLElement | undefined {
  if (state.remoteDialogMode === 'retry' && composerBar.value?.input) {
    return composerBar.value.input;
  }
  return (
    firstRunRemoteProjectButton.value?.button ??
    projectSidebar.value?.openProjectButton
  );
}

/** A send starts with the transcript pinned to its end. */
function handleComposerSend(): void {
  transcriptView.value?.scrollToEnd();
  void sendMessage();
}

function handleExtensionSubmit(value: string | boolean): void {
  void submitExtensionDialog(value);
}

function updateExtensionDraft(value: string): void {
  if (activeExtensionDialog.value) activeExtensionDialog.value.draft = value;
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

  try {
    unlistenQuitRequest = await listen<QuitRequest>(
      QUIT_REQUEST_EVENT,
      ({ payload }) => void handleQuitRequest(payload),
    );
    const pending = await invoke<QuitRequest | null>('pending_quit_request');
    if (pending) await handleQuitRequest(pending);
  } catch {
    // Running in a plain browser, which has no application menu.
  }
}

async function handleQuitRequest(request: QuitRequest): Promise<void> {
  if (!Number.isSafeInteger(request.requestId) || request.requestId <= 0)
    return;
  if (quitRequest.value?.requestId === request.requestId) return;

  const sessionCount = inProgressSessionCount.value;
  if (sessionCount === 0) {
    try {
      await invokeTraced('resolve_quit_request', {
        requestId: request.requestId,
        confirmed: true,
      });
      return;
    } catch {
      quitError.value = 'Tau could not quit. Try again.';
    }
  } else {
    quitError.value = '';
  }

  quitSessionCount.value = sessionCount;
  quitRequest.value = request;
}

function cancelQuit(): void {
  void resolveQuitRequest(false);
}

function confirmQuit(): void {
  void resolveQuitRequest(true);
}

async function resolveQuitRequest(confirmed: boolean): Promise<void> {
  const request = quitRequest.value;
  if (!request || quitBusy.value) return;

  quitBusy.value = true;
  quitError.value = '';
  try {
    const accepted = await invokeTraced<boolean>('resolve_quit_request', {
      requestId: request.requestId,
      confirmed,
    });
    if (!accepted) {
      quitError.value = 'This quit request expired. Press ⌘Q to try again.';
      return;
    }
    quitRequest.value = null;
  } catch {
    quitError.value = confirmed
      ? 'Tau could not quit. Try again.'
      : 'The quit confirmation could not close. Try again.';
  } finally {
    quitBusy.value = false;
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
  if (adminCode.press(event, Date.now())) void toggleAdminMode();
  suppressSystemBeep(event);
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

  /* Pin the single row to the viewport so an overflowing sidebar or session
   * pane scrolls inside itself instead of stretching the window layout. */
  grid-template-rows: minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background: var(--canvas);
}

/* Native chrome dims its selection while its window sits in the background. */
.app-shell.window-inactive {
  --selected: var(--selected-inactive);
}

.app-shell.empty-workspace {
  grid-template-columns: minmax(0, 1fr);
}

.app-shell.resizing-sidebar,
.app-shell.resizing-sidebar * {
  cursor: col-resize;
  /* stylelint-disable-next-line property-no-vendor-prefix -- WKWebView needs the prefix before Safari 17.4 */
  -webkit-user-select: none;
  user-select: none;
}

.app-shell.resizing-sidebar :deep(.sidebar-resize-handle)::after {
  opacity: 0.4;
}

.first-run {
  display: grid;
  grid-template-rows: 30px minmax(0, 1fr);
  min-width: 0;
  min-height: 0;
}

.first-run-titlebar {
  min-height: 30px;
}

.first-run-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 0;
  padding: 24px;
  color: var(--muted);
  text-align: center;
}

.first-run-version {
  display: flex;
  margin: 0 0 14px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: var(--text-xs);
  line-height: var(--leading-ui);
  gap: 8px;
}

.first-run-name {
  color: var(--text);
}

.first-run-version-number {
  color: var(--faint);
}

.first-run-actions {
  display: flex;
  gap: 2px;
}

.first-run-content .first-run-error {
  max-width: 44ch;
  margin: 12px 0 0;
  color: var(--danger);
  font-size: var(--text-sm);
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
  min-width: 0;
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

.empty-session :deep(.composer textarea) {
  flex: 1;
  max-height: none;
  padding-top: 16px;
}
</style>
