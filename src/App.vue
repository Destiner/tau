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
        <p
          v-if="status"
          class="status"
          role="status"
        >
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
              @mouseenter="() => (extensionDialogSelectedIndex = index)"
              @click="() => chooseExtensionDialogOption(option)"
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

          <UiContextMenu
            v-else-if="activeExtensionDialog.method === 'input'"
            :items="() => textFieldItems(() => extensionDialogInput?.input)"
          >
            <UiInput
              ref="extensionDialogInput"
              v-model="activeExtensionDialog.draft"
              type="text"
              autocomplete="off"
              :placeholder="activeExtensionDialog.placeholder"
              :aria-label="activeExtensionDialog.title"
            />
          </UiContextMenu>

          <UiContextMenu
            v-else-if="activeExtensionDialog.method === 'editor'"
            :items="() => textFieldItems(() => extensionDialogInput?.input)"
          >
            <UiTextarea
              ref="extensionDialogInput"
              v-model="activeExtensionDialog.draft"
              rows="6"
              :aria-label="activeExtensionDialog.title"
              @keydown.meta.enter.prevent="handleExtensionDialogSubmit"
              @keydown.ctrl.enter.prevent="handleExtensionDialogSubmit"
            />
          </UiContextMenu>

          <footer class="extension-dialog-actions">
            <template v-if="activeExtensionDialog.method === 'confirm'">
              <UiButton
                ref="extensionDialogPrimaryAction"
                variant="primary"
                type="button"
                @click="acceptExtensionConfirmation"
              >
                Confirm
              </UiButton>
              <UiButton
                variant="secondary"
                type="button"
                @click="rejectExtensionConfirmation"
              >
                No
              </UiButton>
            </template>
            <UiButton
              v-else-if="
                activeExtensionDialog.method === 'input' ||
                activeExtensionDialog.method === 'editor'
              "
              variant="primary"
              type="submit"
            >
              Submit
            </UiButton>
            <UiButton
              variant="secondary"
              type="button"
              @click="cancelExtensionDialog"
            >
              Cancel
            </UiButton>
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
              @mouseenter="() => (commandSelectedIndex = index)"
              @click="() => executeCommand(command)"
            >
              <span class="command-copy">
                <span class="command-name">/{{ command.name }}</span>
                <span
                  v-if="command.description"
                  class="command-description"
                >
                  {{ command.description }}
                </span>
              </span>
              <span class="command-source">{{ command.source }}</span>
            </button>
          </div>
          <UiContextMenu :items="() => textFieldItems(() => composerInput)">
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
          </UiContextMenu>
          <div class="composer-toolbar">
            <span class="composer-selector model-selector">
              <UiSelect
                :model-value="`${currentModelProvider}/${currentModelId}`"
                :options="modelOptions"
                placeholder="Model"
                :fallback-label="currentModelLabel"
                :disabled="settingsDisabled || models.length === 0"
                aria-label="Model"
                @update:model-value="handleModelChange"
              />
            </span>
            <span class="composer-selector effort-selector">
              <UiSelect
                :model-value="currentEffort"
                :options="effortOptions"
                :fallback-label="currentEffortLabel"
                :disabled="settingsDisabled || efforts.length === 0"
                aria-label="Thinking effort"
                :max-width="110"
                @update:model-value="handleEffortChange"
              />
            </span>
            <UiIconButton
              v-if="streaming"
              class="send-button stop"
              size="sm"
              variant="fill"
              tone="danger"
              :disabled="stopping"
              label="Stop Pi"
              @click="stop"
            >
              <UiIcon name="stop" />
            </UiIconButton>
            <UiIconButton
              v-else
              class="send-button"
              size="sm"
              variant="fill"
              :disabled="!canCompose || !draft.trim()"
              label="Send message"
              @click="handleSendMessage"
            >
              <UiIcon name="triangle" />
            </UiIconButton>
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
            <UiIconButton
              size="xs"
              variant="fill"
              label="Dismiss notification"
              @click="() => dismissExtensionNotification(notification.key)"
            >
              <UiIcon name="cross" />
            </UiIconButton>
          </div>
          <MarkdownText
            class="extension-notification-message"
            :source="notification.message"
            :base-path="notification.workingDirectory"
          />
        </div>
      </div>
    </div>

    <UiDialog
      v-model:open="state.remoteDialogOpen"
      :title="
        state.remoteDialogStep === 'connection'
          ? 'SSH connection'
          : 'Choose remote working directory'
      "
      :width="state.remoteDialogStep === 'connection' ? 'sm' : 'md'"
      :busy="state.remoteConnecting"
    >
      <form
        v-if="state.remoteDialogStep === 'connection'"
        @submit.prevent="submitRemoteConnection"
      >
        <UiContextMenu
          :items="() => textFieldItems(() => remoteConnectionInput?.input)"
        >
          <UiInput
            ref="remoteConnectionInput"
            v-model="state.remoteConnectionString"
            variant="mono"
            :error="Boolean(state.remoteConnectionError)"
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
        </UiContextMenu>
      </form>

      <div
        v-else
        class="remote-directory-dialog"
      >
        <UiContextMenu
          :items="() => textFieldItems(() => remoteDirectoryFilterInput?.input)"
        >
          <UiInput
            ref="remoteDirectoryFilterInput"
            v-model="state.remoteDirectoryFilter"
            variant="mono"
            :error="Boolean(state.remoteConnectionError)"
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
        </UiContextMenu>
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
            @mouseenter="() => (state.remoteDirectorySelectedIndex = index)"
            @click="() => chooseRemoteDirectory(option.path, option.kind)"
          >
            {{ option.name }}
          </button>
        </div>
      </div>
    </UiDialog>
  </div>
</template>

<script setup lang="ts">
import { type UnlistenFn, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import Sortable, { type SortableEvent } from 'sortablejs';
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from 'vue';

import SessionHeader from './components/SessionHeader.vue';
import TranscriptView from './components/TranscriptView.vue';
import MarkdownText from './components/ui/MarkdownText.vue';
import UiButton from './components/ui/UiButton.vue';
import UiContextMenu from './components/ui/UiContextMenu.vue';
import UiDialog from './components/ui/UiDialog.vue';
import UiIcon from './components/ui/UiIcon.vue';
import UiIconButton from './components/ui/UiIconButton.vue';
import UiInput from './components/ui/UiInput.vue';
import UiMenu from './components/ui/UiMenu.vue';
import type { UiMenuItem } from './components/ui/UiMenu.vue';
import UiSelect from './components/ui/UiSelect.vue';
import UiSpinner from './components/ui/UiSpinner.vue';
import UiStatusDot from './components/ui/UiStatusDot.vue';
import UiTextarea from './components/ui/UiTextarea.vue';
import type { ProjectSummary, SessionSummary } from './composables/state';
import useTau from './composables/useTau';
import {
  type CommandMenuPlacement,
  commandInvocation,
  commandMenuLayout,
  filterCommands,
  slashCommandQuery,
  CommandOption,
} from './lib/commands';
import type { ThinkingLevel } from './lib/pi/model-scope';

type TextField = HTMLInputElement | HTMLTextAreaElement;

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
const composer = ref<HTMLElement>();
const composerInput = ref<HTMLTextAreaElement>();
const commandMenu = ref<HTMLElement>();
const projectList = ref<HTMLElement>();
const sidebar = ref<HTMLElement>();
const remoteConnectionInput = ref<InstanceType<typeof UiInput>>();
const remoteDirectoryFilterInput = ref<InstanceType<typeof UiInput>>();
const extensionDialogInput = ref<
  InstanceType<typeof UiInput> | InstanceType<typeof UiTextarea>
>();
const extensionDialogPrimaryAction = ref<InstanceType<typeof UiButton>>();
const projectMenuOpen = ref(false);
const windowFocused = ref(true);
const loadingIndicatorVisible = ref(false);
const commandMenuDismissed = ref(false);
const commandSelectedIndex = ref(0);
const commandMenuPlacement = ref<CommandMenuPlacement>('above');
const commandMenuMaxHeight = ref<number>();
const commandMenuOffset = ref(0);
const extensionDialogSelectedIndex = ref(0);
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
  currentModelLabel,
  currentEffortLabel,
  effortLabels,
  settingsDisabled,
  canDraft,
  canCompose,
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
  selectModel,
  selectEffort,
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
const modelOptions = computed(() =>
  models.value.map((model) => ({
    value: `${model.provider}/${model.id}`,
    label: `${model.name} · ${model.provider}`,
  })),
);
const effortOptions = computed(() =>
  efforts.value.map((effort) => ({
    value: effort,
    label: effortLabels[effort],
  })),
);
const commandMenuStyle = computed(() => ({
  maxHeight:
    commandMenuMaxHeight.value === undefined
      ? undefined
      : `${commandMenuMaxHeight.value}px`,
  top:
    commandMenuPlacement.value === 'below'
      ? `${commandMenuOffset.value}px`
      : undefined,
}));
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
  window.addEventListener('resize', updateCommandMenuLayout);
});
onBeforeUnmount(() => {
  projectSortable?.destroy();
  dispose();
  clearTimeout(loadingIndicatorTimer);
  unlistenWindowFocus?.();
  unlistenNewSessionMenu?.();
  document.removeEventListener('contextmenu', handleDocumentContextMenu);
  document.removeEventListener('keydown', handleDocumentKeydown);
  window.removeEventListener('resize', updateCommandMenuLayout);
});

watch(
  () => [state.remoteDialogOpen, state.remoteDialogStep] as const,
  ([open, step]) => {
    if (!open) return;
    void nextTick(() => {
      if (step === 'connection') remoteConnectionInput.value?.input?.focus();
      else remoteDirectoryFilterInput.value?.input?.focus();
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
    } else if (dialog.method === 'select') {
      document.getElementById('extension-dialog-option-0')?.focus();
    } else if (dialog.method === 'confirm') {
      extensionDialogPrimaryAction.value?.button?.focus();
    } else {
      extensionDialogInput.value?.input?.focus();
    }
  });
});

watch(
  () => state.activeControllerKey,
  (controllerKey) => {
    sessionHeader.value?.cancelRename();
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

function focusComposer(): void {
  composerInput.value?.focus();
}

function updateCommandMenuLayout(): void {
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
    topBoundary:
      sessionHeader.value?.header?.getBoundingClientRect().bottom ?? 0,
    bottomBoundary: window.innerHeight,
  });

  commandMenuPlacement.value = layout.placement;
  commandMenuMaxHeight.value = layout.maxHeight;
  commandMenuOffset.value = layout.offset;
}

function handleComposerKeydown(event: KeyboardEvent): void {
  if (commandMenuActive.value && !event.isComposing) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const lastIndex = filteredCommands.value.length - 1;
      if (lastIndex < 0) return;
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      commandSelectedIndex.value = Math.min(
        lastIndex,
        Math.max(0, commandSelectedIndex.value + delta),
      );
      scrollSelectedCommand();
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (selectedCommand.value) executeCommand(selectedCommand.value);
      return;
    }
  }

  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    handleSendMessage();
  }
}

function handleSendMessage(): void {
  transcriptView.value?.scrollToEnd();
  void sendMessage();
}

function executeCommand(command: CommandOption): void {
  draft.value = commandInvocation(command);
  commandSelectedIndex.value = 0;
  handleSendMessage();
}

function scrollSelectedCommand(): void {
  void nextTick(() => {
    document
      .getElementById(`command-option-${commandSelectedIndex.value}`)
      ?.scrollIntoView({ block: 'nearest' });
  });
}

function handleModelChange(value: string): void {
  void selectModel(value);
}

function handleEffortChange(value: string): void {
  void selectEffort(value as ThinkingLevel);
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

/**
 * The menu reads the range when it opens, while the field still holds it: the
 * menu takes focus next, and the items hand that range back on the way out.
 */
function textFieldItems(field: () => TextField | undefined): UiMenuItem[] {
  const element = field();
  const start = element?.selectionStart ?? 0;
  const end = element?.selectionEnd ?? 0;
  const selected = start !== end;
  const editable = element ? !element.readOnly && !element.disabled : false;
  if (!element) return [];
  return [
    {
      label: 'Cut',
      disabled: !selected || !editable,
      run: () => void copyField(element, start, end, editable),
    },
    {
      label: 'Copy',
      disabled: !selected,
      run: () => void copyField(element, start, end, false),
    },
    {
      label: 'Paste',
      disabled: !editable,
      run: () => void pasteField(element, start, end),
    },
  ];
}

async function copyField(
  field: TextField,
  start: number,
  end: number,
  cut: boolean,
): Promise<void> {
  const text = field.value.slice(start, end);
  if (!text) return;
  try {
    await writeText(text);
  } catch {
    return;
  }
  if (cut) replaceFieldRange(field, start, end, '');
}

async function pasteField(
  field: TextField,
  start: number,
  end: number,
): Promise<void> {
  let text: string;
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
): void {
  field.value = field.value.slice(0, start) + text + field.value.slice(end);
  const caret = start + text.length;
  field.dispatchEvent(new Event('input', { bubbles: true }));
  field.focus();
  field.setSelectionRange(caret, caret);
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
  } else if (state.remoteDialogOpen) closeRemoteProjectDialog();
  else if (commandMenuActive.value) {
    event.preventDefault();
    commandMenuDismissed.value = true;
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

function chooseExtensionDialogOption(value: string): void {
  void submitExtensionDialog(value);
}

function acceptExtensionConfirmation(): void {
  void submitExtensionDialog(true);
}

function rejectExtensionConfirmation(): void {
  void submitExtensionDialog(false);
}

function handleExtensionDialogSubmit(): void {
  const dialog = activeExtensionDialog.value;
  if (!dialog || (dialog.method !== 'input' && dialog.method !== 'editor')) {
    return;
  }
  void submitExtensionDialog(dialog.draft);
}

function handleExtensionSelectKeydown(event: KeyboardEvent): void {
  const dialog = activeExtensionDialog.value;
  const options = dialog?.options ?? [];
  if (dialog?.method !== 'select' || options.length === 0) return;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
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

function handleLocalProject(): void {
  void addLocalProject();
}

function handleRemoteProject(): void {
  openRemoteProjectDialog();
}

function handleRemoteDirectoryKeydown(event: KeyboardEvent): void {
  if (state.remoteConnecting) return;
  const lastIndex = remoteDirectoryOptions.value.length - 1;
  if (lastIndex < 0) return;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    state.remoteDirectorySelectedIndex = Math.min(
      lastIndex,
      Math.max(0, state.remoteDirectorySelectedIndex + delta),
    );
    scrollSelectedRemoteDirectory();
    return;
  }
  if (event.key === 'Enter' && !event.isComposing) {
    event.preventDefault();
    const option =
      remoteDirectoryOptions.value[state.remoteDirectorySelectedIndex];
    if (option) void chooseRemoteDirectory(option.path, option.kind);
  }
}

function scrollSelectedRemoteDirectory(): void {
  void nextTick(() => {
    document
      .getElementById(
        `remote-directory-option-${state.remoteDirectorySelectedIndex}`,
      )
      ?.scrollIntoView({ block: 'nearest' });
  });
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
