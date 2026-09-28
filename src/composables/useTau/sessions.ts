import { errorCopy } from '../../lib/error-copy';
import {
  discardUnregisteredEphemeralSession,
  persistExpandedProject,
  persistProjectSelection,
  releaseIdleRuntimes,
  releaseRuntime,
  removeEmptyActivePhantom,
  removeRegisteredEphemeralSession,
  startController,
} from '../../lib/pi/runtime';
import { startActionMilestones } from '../../lib/telemetry';
import { advanceArchiveRevision, archiveRevision } from '../archive-revision';
import {
  activeController,
  projectActionsDisabled,
  canArchiveSession,
  clearActiveSession,
  clearControllerFeedback,
  controllerForSession,
  createController,
  createPhantomSession,
  ensureController,
  inheritControllerSettings,
  isSessionSelected,
  nextControllerKey,
  projectSessions,
  reopenRemoteFeedback,
  runtimeAvailable,
  setActiveSessionView,
  setControllerError,
  setWorkspaceError,
  state,
  type ProjectSummary,
  type SessionController,
  type SessionSummary,
} from '../state';

import {
  cancelSavedPreview,
  trackSelection,
  trackSelectionWrite,
  readSavedSession,
} from './navigation';
import controllerTelemetryScope from './telemetry-scope';
import { writeWorkspace, archiveIntent } from './workspace-writes';

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function createSessions() {
  async function archiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (!canArchiveSession(project, session)) return;
    const controller = controllerForSession(project.path, session.id);
    clearControllerFeedback(controller, errorCopy.archiveSession);
    const discardedViewWasSelected =
      state.activeProjectPath === project.path &&
      state.activeSessionId === session.id;
    if (controller && discardUnregisteredEphemeralSession(controller)) {
      if (!discardedViewWasSelected) return;
      const updatedProject = state.workspace?.projects.find(
        (candidate) => candidate.path === project.path,
      );
      if (!updatedProject) {
        clearActiveSession();
        return;
      }
      const nextSession = projectSessions(updatedProject)[0];
      if (nextSession) await selectSession(updatedProject, nextSession);
      else await newSession(updatedProject);
      return;
    }

    const intent = archiveIntent.get(session) ?? {
      confirmed: session.archived,
    };
    archiveIntent.set(session, intent);
    const revision = advanceArchiveRevision(project.path, session.id);
    session.archived = true;
    session.selected = false;
    if (controller) removeRegisteredEphemeralSession(controller);
    if (discardedViewWasSelected) {
      const nextSession = projectSessions(project)[0];
      if (nextSession) void selectSession(project, nextSession);
      else void newSession(project);
    }
    try {
      await writeWorkspace('archive_session', {
        projectPath: project.path,
        sessionId: session.id,
      });
      intent.confirmed = true;
      const archivedViewStillSelected =
        state.activeProjectPath === project.path &&
        state.activeSessionId === session.id;
      if (!archivedViewStillSelected) releaseRuntime(controller);
    } catch {
      if (archiveRevision(project.path, session.id) === revision)
        session.archived = intent.confirmed;
      const errorController = controller ?? ensureController(project, session);
      setControllerError(errorController, errorCopy.archiveSession);
      if (!isSessionSelected(project, session)) errorController.unread = true;
    }
  }

  async function unarchiveSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    const intent = archiveIntent.get(session) ?? {
      confirmed: session.archived,
    };
    archiveIntent.set(session, intent);
    const revision = advanceArchiveRevision(project.path, session.id);
    session.archived = false;
    try {
      await writeWorkspace('unarchive_session', {
        projectPath: project.path,
        sessionId: session.id,
      });
      intent.confirmed = false;
    } catch {
      if (archiveRevision(project.path, session.id) === revision)
        session.archived = intent.confirmed;
      setWorkspaceError(errorCopy.restoreSession);
    }
  }

  async function newSession(
    project: ProjectSummary,
    bootstrap = false,
  ): Promise<SessionController | undefined> {
    if (!bootstrap && projectActionsDisabled.value) return;
    const milestones = startActionMilestones('session.new');
    const actionSpan = milestones.span;
    try {
      const previous = activeController.value;
      removeEmptyActivePhantom();
      const controllerKey = nextControllerKey();
      const session = createPhantomSession(project.path, controllerKey);
      const controller = createController(project, session, controllerKey);
      inheritControllerSettings(controller, previous);
      state.ephemeralSessions.push(session);
      if (project.collapsed) {
        project.collapsed = false;
        void persistExpandedProject(project.path, actionSpan.context);
      }
      setActiveSessionView(project, session, controller);
      cancelSavedPreview();
      trackSelection(controller, milestones, project.path, session.id);
      trackSelectionWrite(
        persistProjectSelection(project.path, controller, actionSpan.context),
        milestones,
      );
      releaseIdleRuntimes();
      if (
        runtimeAvailable(project) &&
        (!controller.currentModelId ||
          controller.models.length === 0 ||
          controller.efforts.length === 0 ||
          !controller.commandsLoaded)
      ) {
        await startController(
          controller,
          project,
          undefined,
          false,
          actionSpan.context,
        );
      }
      return controller;
    } finally {
      actionSpan.end();
    }
  }

  async function selectSession(
    project: ProjectSummary,
    session: SessionSummary,
  ): Promise<void> {
    if (projectActionsDisabled.value) return;
    const milestones = startActionMilestones(
      'session.select',
      controllerTelemetryScope(
        controllerForSession(project.path, session.id),
        session.id,
      ),
    );
    const actionSpan = milestones.span;
    try {
      const alreadySelected =
        project.path === state.activeProjectPath &&
        session.id === state.activeSessionId;
      if (alreadySelected) {
        const controller = activeController.value;
        if (controller) {
          controller.unread = false;
          if (controller.remoteDisconnected) reopenRemoteFeedback(controller);
          if (
            !controller.phantom &&
            !controller.ready &&
            !controller.starting &&
            !controller.remoteDisconnected
          ) {
            readSavedSession(project, session, controller);
            await startController(
              controller,
              project,
              session.path,
              false,
              actionSpan.context,
            );
          }
        }
        return;
      }

      removeEmptyActivePhantom();
      const controller = ensureController(project, session);
      setActiveSessionView(project, session, controller);
      if (
        !session.path ||
        controller.messagesLoaded ||
        controller.messages.length
      )
        cancelSavedPreview();
      if (controller.remoteDisconnected) reopenRemoteFeedback(controller);
      controller.unread = false;
      trackSelection(controller, milestones, project.path, session.id);
      trackSelectionWrite(
        persistProjectSelection(project.path, controller, actionSpan.context),
        milestones,
      );
      releaseIdleRuntimes();

      if (controller.ready && !controller.messagesLoaded)
        readSavedSession(project, session, controller);
      if (
        controller.phantom ||
        controller.ready ||
        controller.starting ||
        controller.remoteDisconnected
      )
        return;
      readSavedSession(project, session, controller);
      await startController(
        controller,
        project,
        session.path,
        false,
        actionSpan.context,
      );
    } finally {
      actionSpan.end();
    }
  }
  return { archiveSession, unarchiveSession, newSession, selectSession };
}

export default createSessions;
