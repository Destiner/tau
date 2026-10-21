import { open } from '@tauri-apps/plugin-dialog';

import { errorCopy } from '../../lib/error-copy';
import {
  removeProjectUiState,
  stopControllerProcess,
} from '../../lib/pi/runtime';
import { invokeTraced } from '../../lib/telemetry';
import {
  projectActionsDisabled,
  clearActiveSession,
  projectSessions,
  setWorkspaceError,
  state,
  type ProjectSummary,
  type SessionController,
  type SessionSummary,
  type WorkspaceSnapshot,
} from '../state';

import { beginNavigation, isCurrentNavigation } from './navigation';
import {
  writeWorkspace,
  foldIntent,
  nextOrderRevision,
  isCurrentOrderRevision,
} from './workspace-writes';

function mergeImportedProject(
  project: ProjectSummary,
): ProjectSummary | undefined {
  const workspace = state.workspace;
  if (!workspace) return;
  const existing = workspace.projects.find(
    (item) => item.path === project.path,
  );
  if (existing) {
    for (const session of project.sessions) {
      if (!existing.sessions.some((item) => item.id === session.id))
        existing.sessions.push({ ...session, selected: false });
    }
    return existing;
  }
  workspace.projects.push({ ...project, selected: false });
  return workspace.projects[workspace.projects.length - 1];
}

let localOpening = false;

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function createProjects(
  selectSession: (
    project: ProjectSummary,
    session: SessionSummary,
  ) => Promise<void>,
  newSession: (
    project: ProjectSummary,
    bootstrap?: boolean,
  ) => Promise<SessionController | undefined>,
) {
  function activateProject(project: ProjectSummary): void {
    const target = state.workspace?.projects.find(
      (item) => item.path === project.path,
    );
    if (!target || projectActionsDisabled.value) return;
    const sessions = projectSessions(target).filter(
      (session) => !session.archived,
    );
    const chosen =
      sessions.find(
        (session) =>
          target.path === state.activeProjectPath &&
          session.id === state.activeSessionId,
      ) ??
      sessions.find((session) => session.selected) ??
      sessions[0];
    if (target.collapsed) void toggleProject(target);
    if (chosen) void selectSession(target, chosen);
    else void newSession(target);
  }

  async function addLocalProject(): Promise<void> {
    if (projectActionsDisabled.value || localOpening) return;
    const intent = beginNavigation();
    localOpening = true;
    let selection: string | null;
    try {
      selection = await open({
        directory: true,
        multiple: false,
        title: 'Choose a Project Folder',
      });
    } catch {
      if (isCurrentNavigation(intent))
        setWorkspaceError(errorCopy.folderPicker);
      return;
    } finally {
      localOpening = false;
    }
    if (!selection || !isCurrentNavigation(intent)) return;
    try {
      const imported = await writeWorkspace<ProjectSummary>('import_project', {
        path: selection,
      });
      if (!isCurrentNavigation(intent)) return;
      if (state.workspace && !state.workspace.piPath) {
        const workspace = await invokeTraced<WorkspaceSnapshot>(
          'load_workspace',
          {},
        );
        if (!isCurrentNavigation(intent)) return;
        if (state.workspace) state.workspace.piPath = workspace.piPath;
      }
      const project = mergeImportedProject(imported);
      if (project) activateProject(project);
    } catch {
      if (isCurrentNavigation(intent))
        setWorkspaceError(errorCopy.importProject);
    }
  }

  async function toggleProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    const previous = foldIntent.get(project);
    const intent = {
      revision: (previous?.revision ?? 0) + 1,
      confirmed: previous?.confirmed ?? project.collapsed,
    };
    foldIntent.set(project, intent);
    const collapsed = !project.collapsed;
    project.collapsed = collapsed;
    try {
      await writeWorkspace('set_project_collapsed', {
        path: project.path,
        collapsed,
      });
      intent.confirmed = collapsed;
      if (foldIntent.get(project)?.revision !== intent.revision)
        foldIntent.get(project)!.confirmed = collapsed;
    } catch {
      if (foldIntent.get(project)?.revision === intent.revision)
        project.collapsed = foldIntent.get(project)!.confirmed;
      setWorkspaceError(errorCopy.sidebarChange);
    }
  }

  async function reorderProjects(
    fromIndex: number,
    toIndex: number,
  ): Promise<void> {
    const workspace = state.workspace;
    if (
      !workspace ||
      projectActionsDisabled.value ||
      fromIndex < 0 ||
      fromIndex >= workspace.projects.length ||
      toIndex < 0 ||
      toIndex >= workspace.projects.length ||
      fromIndex === toIndex
    ) {
      return;
    }

    const projects = [...workspace.projects];
    const [project] = projects.splice(fromIndex, 1);
    if (!project) return;
    projects.splice(toIndex, 0, project);
    state.workspace = { ...workspace, projects };
    const revision = nextOrderRevision();

    try {
      await writeWorkspace('reorder_projects', {
        projectPaths: projects.map((candidate) => candidate.path),
      });
    } catch {
      if (isCurrentOrderRevision(revision) && state.workspace)
        state.workspace.projects = workspace.projects;
      setWorkspaceError(errorCopy.projectOrder);
    }
  }

  async function removeProject(project: ProjectSummary): Promise<void> {
    if (projectActionsDisabled.value) return;
    beginNavigation();
    state.removingProjectPaths.push(project.path);
    const projectControllers = state.controllers.filter(
      (controller) => controller.projectPath === project.path,
    );

    try {
      const removingActiveView = project.path === state.activeProjectPath;
      await writeWorkspace('remove_project', { path: project.path });
      const currentWorkspace = state.workspace;
      if (currentWorkspace) {
        currentWorkspace.projects = currentWorkspace.projects.filter(
          (candidate) => candidate.path !== project.path,
        );
        if (currentWorkspace.activeProjectPath === project.path)
          currentWorkspace.activeProjectPath = '';
      }
      if (removingActiveView && state.activeProjectPath === project.path) {
        clearActiveSession();
      }
      if (!state.activeProjectPath) {
        state.activeProjectPath = currentWorkspace?.activeProjectPath ?? '';
      }
      for (const controller of projectControllers) controller.disposed = true;
      await Promise.all(
        projectControllers.map((controller) =>
          stopControllerProcess(controller),
        ),
      );
      removeProjectUiState(project.path);
    } catch {
      setWorkspaceError(errorCopy.removeProject);
    } finally {
      state.removingProjectPaths = state.removingProjectPaths.filter(
        (path) => path !== project.path,
      );
    }
  }
  return {
    activateProject,
    addLocalProject,
    toggleProject,
    reorderProjects,
    removeProject,
  };
}

export { mergeImportedProject, createProjects };
