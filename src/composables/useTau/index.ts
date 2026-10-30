import { computed } from 'vue';

import {
  invokesExtensionCommand,
  retrySetup as retryControllerSetup,
} from '../../lib/pi/runtime';
import {
  activeController,
  activeExtensionDialog,
  activeFeedback,
  activeProject,
  projectActionsDisabled,
  canArchiveSession,
  canCompose,
  canDraft,
  canQueue,
  compactionRestriction,
  activeQueue,
  queueBusy,
  queueFeedback,
  queueFailedDrafts,
  canRecoverRuntime,
  canRenameSession,
  acknowledgeFeedback,
  commands,
  compacting,
  currentEffort,
  currentEffortLabel,
  currentModelId,
  currentModelLabel,
  currentModelProvider,
  draft,
  effortLabels,
  efforts,
  indicatorLabel,
  inProgressSessionCount,
  isProjectRemoving,
  isSessionSelected,
  isSessionUnread,
  markSessionRead,
  markSessionUnread,
  messages,
  models,
  projectIndicator,
  projectSessions,
  promptSubmitting,
  archivedSessionEntries,
  relativeTimestamp,
  retryPresentation,
  sessionLastUserMessageAt,
  sessionSortAt,
  sessionIndicator,
  sessionTooltipStatus,
  expandedRelativeTime,
  sessionLastActive,
  sessionLoading,
  sessionTitle,
  settingsDisabled,
  state,
  stopping,
  streaming,
} from '../state';

import createComposer from './composer';
import createLifecycle from './lifecycle';
import {
  openProjectRename,
  closeProjectRename,
  submitProjectRename,
} from './project-rename';
import { createProjects } from './projects';
import {
  openRemoteProjectDialog,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  createRemote,
  recoverSession,
} from './remote';
import createSessions from './sessions';
import { selectModel, renameSession, selectEffort } from './settings';

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function useTau() {
  const { archiveSession, unarchiveSession, newSession, selectSession } =
    createSessions();
  const {
    activateProject,
    addLocalProject,
    toggleProject,
    reorderProjects,
    requestProjectRemoval,
    cancelProjectRemoval,
    confirmProjectRemoval,
  } = createProjects(selectSession, newSession);
  const { chooseRemoteDirectory } = createRemote(activateProject);
  const { initialize, dispose } = createLifecycle(newSession);
  const {
    sendMessage,
    clearQueue,
    dismissQueueFeedback,
    recoverQueueDraft,
    stop,
    loadEarlierHistory,
    submitExtensionDialog,
    cancelExtensionDialog,
  } = createComposer(unarchiveSession);
  const extensionCommandDraft = computed(() => {
    const controller = activeController.value;
    return Boolean(
      controller && invokesExtensionCommand(controller, draft.value.trim()),
    );
  });
  return {
    state,
    activeProject,
    projectActionsDisabled,
    activeController,
    messages,
    draft,
    retryPresentation,
    activeFeedback,
    acknowledgeFeedback,
    streaming,
    compacting,
    stopping,
    promptSubmitting,
    inProgressSessionCount,
    models,
    efforts,
    commands,
    activeExtensionDialog,
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
    canQueue,
    compactionRestriction,
    activeQueue,
    queueBusy,
    queueFeedback,
    queueFailedDrafts,
    extensionCommandDraft,
    dismissQueueFeedback,
    canRecoverRuntime,
    canRenameSession,
    sessionLoading,
    initialize,
    dispose,
    activateProject,
    addLocalProject,
    openRemoteProjectDialog,
    closeRemoteProjectDialog,
    submitRemoteConnection,
    chooseRemoteDirectory,
    toggleProject,
    reorderProjects,
    requestProjectRemoval,
    cancelProjectRemoval,
    confirmProjectRemoval,
    archiveSession,
    unarchiveSession,
    newSession,
    selectSession,
    canArchiveSession,
    isProjectRemoving,
    projectSessions,
    archivedSessionEntries,
    sessionLastActive,
    sessionLastUserMessageAt,
    sessionSortAt,
    relativeTimestamp,
    isSessionSelected,
    sessionIndicator,
    sessionTooltipStatus,
    expandedRelativeTime,
    isSessionUnread,
    markSessionUnread,
    markSessionRead,
    projectIndicator,
    indicatorLabel,
    sendMessage,
    clearQueue,
    recoverQueueDraft,
    recoverSession,
    stop,
    loadEarlierHistory,
    submitExtensionDialog,
    cancelExtensionDialog,
    renameSession,
    openProjectRename,
    closeProjectRename,
    submitProjectRename,
    setupFailure: computed(() => activeController.value?.setupFailure),
    canRetrySetup: computed(() => {
      const controller = activeController.value;
      return Boolean(
        controller?.generation &&
        controller.ready &&
        !controller.starting &&
        !controller.setupFailure?.recovering,
      );
    }),
    retrySetup: async (): Promise<void> => {
      const controller = activeController.value;
      if (controller) await retryControllerSetup(controller);
    },
    selectModel,
    selectEffort,
  };
}

export default useTau;
