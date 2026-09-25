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
        <div
          v-if="preparationIndicatorVisible"
          class="first-run-preparing"
          role="status"
        >
          <UiSpinner label="Preparing Pi" />
          <span>Preparing Pi</span>
        </div>
        <template v-if="workspaceIsEmpty">
          <UpdateStatus first-run />
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
          'has-queue':
            queueVisible && !activeExtensionDialog && !remoteReconnectFeedback,
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
          :remote-project-path="
            activeProject?.connectionString ? activeProject.path : undefined
          "
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
        <MessageQueue
          v-if="
            queueVisible &&
            !sessionLoading &&
            !activeExtensionDialog &&
            !remoteReconnectFeedback
          "
          :queue="activeQueue"
          :feedback="queueFeedback"
          :busy="queueBusy"
          :failed-drafts="queueFailedDrafts"
          :can-recover="!activeController?.draft"
          @clear="handleQueueClear"
          @recover="handleQueueRecover"
        />
        <footer
          v-if="!sessionLoading && !activeExtensionDialog"
          class="composer-area"
        >
          <ReconnectStatus
            v-if="remoteReconnectFeedback"
            :message="remoteReconnectFeedback.message"
            :busy="feedbackActionBusy"
            :disabled="!activeController?.remoteDisconnected"
            @reconnect="handleRemoteReconnect"
          />
          <ComposerBar
            v-else
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
      :title="quitTitle"
      :action="quitAction"
      @cancel="cancelQuit"
      @confirm="confirmQuit"
    />

    <FeedbackDialog
      :key="activeFeedback?.id"
      :open="feedbackDialogOpen"
      :incident="activeFeedback"
      :busy="feedbackActionBusy"
      :return-focus="feedbackReturnFocus"
      @action="handleFeedbackAction"
      @close="closeFeedback"
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
import FeedbackDialog from './components/FeedbackDialog.vue';
import MessageQueue from './components/MessageQueue.vue';
import ProjectSidebar from './components/ProjectSidebar.vue';
import QuitConfirmation from './components/QuitConfirmation.vue';
import ReconnectStatus from './components/ReconnectStatus.vue';
import RemoteDialog from './components/RemoteDialog.vue';
import SessionHeader from './components/SessionHeader.vue';
import TranscriptView from './components/TranscriptView.vue';
import UpdateStatus from './components/UpdateStatus.vue';
import UiButton from './components/ui/UiButton.vue';
import UiSpinner from './components/ui/UiSpinner.vue';
import useTau from './composables/useTau';
import { createAdminCodeMatcher } from './lib/admin-code';
import { toggleAdminMode } from './lib/admin-mode';
import { loadSidebarWidth } from './lib/sidebar-width';
import { invokeTraced } from './lib/telemetry';
import { useUpdate } from './lib/update';

const NEW_SESSION_EVENT = 'tau://new-session';
const QUIT_REQUEST_EVENT = 'tau://quit-requested';
const FULLSCREEN_VIEWER_STATE_EVENT = 'tau:fullscreen-viewer-state';
/** How long a session may hydrate before it is worth reporting as loading. */
const LOADING_INDICATOR_DELAY_MS = 200;
const PREPARATION_INDICATOR_DELAY_MS = 200;
const EDITABLE_SELECTOR = 'input, textarea, select';

interface QuitRequest {
  requestId: number;
  intent?: 'ordinary' | 'updateRestart';
  operationId?: number;
}

const transcriptView = ref<InstanceType<typeof TranscriptView>>();
const projectSidebar = ref<InstanceType<typeof ProjectSidebar>>();
const sessionHeader = ref<InstanceType<typeof SessionHeader>>();
const composerBar = ref<InstanceType<typeof ComposerBar>>();
const firstRunLocalProjectButton = ref<InstanceType<typeof UiButton>>();
const firstRunRemoteProjectButton = ref<InstanceType<typeof UiButton>>();
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const preparationIndicatorVisible = ref(false);
const sidebarWidth = ref(loadSidebarWidth());
const resizingSidebar = ref(false);
const quitRequest = ref<QuitRequest | null>(null);
const quitSessionCount = ref(0);
const quitBusy = ref(false);
const quitError = ref('');
const feedbackFocusTarget = ref<HTMLElement>();
const fullscreenViewerOpen = ref(false);
const adminCode = createAdminCodeMatcher(isEditableTarget);
const update = useUpdate();
let unlistenWindowFocus: UnlistenFn | undefined;
let unlistenNewSessionMenu: UnlistenFn | undefined;
let unlistenQuitRequest: UnlistenFn | undefined;
let loadingIndicatorTimer: ReturnType<typeof setTimeout> | undefined;
let preparationIndicatorTimer: ReturnType<typeof setTimeout> | undefined;
const {
  state,
  activeController,
  activeFeedback,
  activeProject,
  projectActionsDisabled,
  messages,
  canDraft,
  activeQueue,
  queueFeedback,
  queueBusy,
  queueFailedDrafts,
  clearQueue,
  recoverQueueDraft,
  streaming,
  compacting,
  stopping,
  promptSubmitting,
  inProgressSessionCount,
  activeExtensionDialog,
  sessionLoading,
  initialize,
  acknowledgeFeedback,
  addLocalProject,
  openRemoteProjectDialog,
  dispose,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  newSession,
  sendMessage,
  reconnectRemoteSession,
  loadEarlierHistory,
  submitExtensionDialog,
  cancelExtensionDialog,
} = useTau();

/**
 * An indicator without a state still reserves its slot, so it stays hidden
 * from assistive tech until it carries a meaning worth announcing. UiStatusDot
 * renders that itself from the label.
 */

const remoteReconnectFeedback = computed(() =>
  activeFeedback.value?.action === 'reconnect'
    ? activeFeedback.value
    : undefined,
);
const feedbackDialogOpen = computed(
  () =>
    Boolean(activeFeedback.value) &&
    !remoteReconnectFeedback.value &&
    quitRequest.value === null &&
    !state.remoteDialogOpen &&
    !activeExtensionDialog.value &&
    !fullscreenViewerOpen.value,
);
const quitTitle = computed(() =>
  quitRequest.value?.intent === 'updateRestart' ? 'Restart Tau?' : 'Quit Tau?',
);
const quitAction = computed(() =>
  quitRequest.value?.intent === 'updateRestart' ? 'Restart' : 'Quit',
);
const feedbackActionBusy = computed(() => {
  switch (activeFeedback.value?.action) {
    case 'initialize':
      return state.initializing;
    case 'reconnect':
      return Boolean(
        activeController.value?.reconnectingRemote ||
        activeController.value?.starting,
      );
    default:
      return false;
  }
});
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
const queueVisible = computed(() =>
  Boolean(
    activeQueue.value.steering.length ||
    activeQueue.value.followUp.length ||
    queueFeedback.value ||
    queueFailedDrafts.value.length,
  ),
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
  update.initialize();
  void initialize();
  void watchWindowFocus();
  void watchMenuActions();
  document.addEventListener('contextmenu', handleDocumentContextMenu);
  document.addEventListener('keydown', handleDocumentKeydown);
  window.addEventListener(
    FULLSCREEN_VIEWER_STATE_EVENT,
    handleFullscreenViewerState,
  );
});
onBeforeUnmount(() => {
  update.dispose();
  dispose();
  clearTimeout(loadingIndicatorTimer);
  clearTimeout(preparationIndicatorTimer);
  unlistenWindowFocus?.();
  unlistenNewSessionMenu?.();
  unlistenQuitRequest?.();
  document.removeEventListener('contextmenu', handleDocumentContextMenu);
  document.removeEventListener('keydown', handleDocumentKeydown);
  window.removeEventListener(
    FULLSCREEN_VIEWER_STATE_EVENT,
    handleFullscreenViewerState,
  );
});

watch(
  () => state.initializing && state.workspace === null,
  (preparing) => {
    clearTimeout(preparationIndicatorTimer);
    preparationIndicatorVisible.value = false;
    if (!preparing) return;
    preparationIndicatorTimer = setTimeout(() => {
      preparationIndicatorVisible.value = true;
    }, PREPARATION_INDICATOR_DELAY_MS);
  },
  { immediate: true },
);

watch(workspaceIsEmpty, (empty) => {
  if (!empty) return;
  void nextTick(() => firstRunLocalProjectButton.value?.button?.focus());
});

watch(activeExtensionDialog, (dialog) => {
  void nextTick(() => {
    if (!dialog && !activeFeedback.value && !fullscreenViewerOpen.value) {
      composerBar.value?.focus();
    }
  });
});

watch(activeFeedback, (incident, previous) => {
  if (incident && incident.id !== previous?.id) {
    const focused = document.activeElement;
    feedbackFocusTarget.value =
      focused instanceof HTMLElement ? focused : undefined;
    return;
  }
  if (!incident && previous?.action === 'reconnect') {
    void nextTick(() => composerBar.value?.focus());
  }
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
      if (
        !state.remoteDialogOpen &&
        !activeExtensionDialog.value &&
        !activeFeedback.value &&
        !fullscreenViewerOpen.value
      ) {
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
    if (
      !state.remoteDialogOpen &&
      !activeExtensionDialog.value &&
      !activeFeedback.value &&
      !fullscreenViewerOpen.value
    ) {
      composerBar.value?.focus();
    }
  });
});

watch(promptSubmitting, () => {
  void nextTick(() => {
    if (
      !state.remoteDialogOpen &&
      !activeExtensionDialog.value &&
      !activeFeedback.value &&
      !fullscreenViewerOpen.value
    ) {
      composerBar.value?.focus();
    }
  });
});

function handleFullscreenViewerState(event: Event): void {
  fullscreenViewerOpen.value = Boolean((event as CustomEvent<unknown>).detail);
}

function closeFeedback(): void {
  if (activeFeedback.value) acknowledgeFeedback(activeFeedback.value);
}

function feedbackReturnFocus(): HTMLElement | undefined {
  const previous = feedbackFocusTarget.value;
  if (previous?.isConnected) return previous;
  return (
    composerBar.value?.input ??
    firstRunLocalProjectButton.value?.button ??
    projectSidebar.value?.openProjectButton
  );
}

async function handleRemoteReconnect(): Promise<void> {
  await reconnectRemoteSession();
}

async function handleFeedbackAction(): Promise<void> {
  const incident = activeFeedback.value;
  if (!incident?.action) return;
  if (incident.action === 'reconnect') {
    await handleRemoteReconnect();
    return;
  }
  acknowledgeFeedback(incident);
  if (incident.action === 'initialize') {
    await initialize();
    return;
  }
  window.location.reload();
}

function focusComposer(): void {
  if (!activeFeedback.value && !fullscreenViewerOpen.value)
    composerBar.value?.focus();
}

async function handleQueueClear(): Promise<void> {
  await clearQueue();
  if (!queueVisible.value) {
    await nextTick();
    focusComposer();
  }
}

function handleQueueRecover(index: number): void {
  recoverQueueDraft(index);
  void nextTick(focusComposer);
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
function handleComposerSend(intent: 'steer' | 'followUp'): void {
  if (!streaming.value) transcriptView.value?.scrollToEnd();
  void sendMessage(intent);
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
        if (payload) update.handleFocus();
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
  if (
    request.intent === 'updateRestart' &&
    (!Number.isSafeInteger(request.operationId) ||
      (request.operationId ?? 0) <= 0)
  )
    return;
  if (quitRequest.value?.requestId === request.requestId) return;

  const sessionCount = inProgressSessionCount.value;
  if (sessionCount === 0) {
    try {
      const accepted = await invokeTraced<boolean>('resolve_quit_request', {
        requestId: request.requestId,
        confirmed: true,
      });
      if (accepted) {
        if (
          request.intent === 'updateRestart' &&
          request.operationId !== undefined
        ) {
          await update.install(request.requestId, request.operationId);
        }
        return;
      }
    } catch {
      quitError.value =
        request.intent === 'updateRestart'
          ? 'Tau could not restart. Try again.'
          : 'Tau could not quit. Try again.';
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
    if (
      confirmed &&
      request.intent === 'updateRestart' &&
      request.operationId !== undefined
    ) {
      await update.install(request.requestId, request.operationId);
    }
    quitRequest.value = null;
  } catch {
    quitError.value = confirmed
      ? request.intent === 'updateRestart'
        ? 'Tau could not restart. Try again.'
        : 'Tau could not quit. Try again.'
      : 'The confirmation could not close. Try again.';
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

.first-run-actions {
  display: flex;
  gap: 2px;
}

.first-run-preparing {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: var(--muted);
  font-size: var(--text-sm);
}

.session-pane {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  min-width: 0;
  min-height: 0;
}

.session-pane.has-queue:not(.empty-session, .loading-session) {
  grid-template-rows: auto minmax(0, 1fr) auto auto;
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
