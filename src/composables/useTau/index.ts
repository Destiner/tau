import { computed } from 'vue';

import { invokesExtensionCommand } from '../../lib/pi/runtime';
import { invokeTraced } from '../../lib/telemetry';
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
  activeQueue,
  queueBusy,
  queueFeedback,
  queueFailedDrafts,
  canReconnectRemote,
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
import { createProjects } from './projects';
import {
  openRemoteProjectDialog,
  closeRemoteProjectDialog,
  submitRemoteConnection,
  chooseRemoteDirectory,
  reconnectRemoteSession,
} from './remote';
import createSessions from './sessions';
import { selectModel, renameSession, selectEffort } from './settings';

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function useTau() {
  const { archiveSession, unarchiveSession, newSession, selectSession } =
    createSessions();
  const { addLocalProject, toggleProject, reorderProjects, removeProject } =
    createProjects(selectSession, newSession);
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
  function submitIssueReport(
    description: string,
    sessionId?: string,
  ): Promise<void> {
    return invokeTraced('submit_issue_report', {
      description,
      ...(sessionId ? { sessionId } : {}),
    });
  }

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
    activeQueue,
    queueBusy,
    queueFeedback,
    queueFailedDrafts,
    extensionCommandDraft,
    dismissQueueFeedback,
    canReconnectRemote,
    canRenameSession,
    sessionLoading,
    initialize,
    dispose,
    submitIssueReport,
    addLocalProject,
    openRemoteProjectDialog,
    closeRemoteProjectDialog,
    submitRemoteConnection,
    chooseRemoteDirectory,
    toggleProject,
    reorderProjects,
    removeProject,
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
    reconnectRemoteSession,
    stop,
    loadEarlierHistory,
    submitExtensionDialog,
    cancelExtensionDialog,
    renameSession,
    selectModel,
    selectEffort,
  };
}

export default useTau;
