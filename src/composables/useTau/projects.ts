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

import {
  writeWorkspace,
  foldIntent,
  nextOrderRevision,
  isCurrentOrderRevision,
} from './workspace-writes';

function mergeImportedProject(project: ProjectSummary): void {
  const workspace = state.workspace;
  if (!workspace) return;
  const index = workspace.projects.findIndex(
    (item) => item.path === project.path,
  );
  if (index < 0) workspace.projects.push(project);
  else workspace.projects.splice(index, 1, project);
  if (project.selected) {
    workspace.activeProjectPath = project.path;
    for (const item of workspace.projects)
      item.selected = item.path === project.path;
  }
}

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
  async function addLocalProject(): Promise<void> {
    if (projectActionsDisabled.value) return;
    let selection: string | null;
    try {
      selection = await open({
        directory: true,
        multiple: false,
        title: 'Choose a Project Folder',
      });
    } catch {
      setWorkspaceError(errorCopy.folderPicker);
      return;
    }
    if (!selection) return;
    try {
      mergeImportedProject(
        await invokeTraced<ProjectSummary>('import_project', {
          path: selection,
        }),
      );
      if (state.workspace && !state.workspace.piPath) {
        state.workspace.piPath = (
          await invokeTraced<WorkspaceSnapshot>('load_workspace', {})
        ).piPath;
      }
      if (!state.activeProjectPath) {
        state.activeProjectPath = state.workspace?.activeProjectPath ?? '';
      }
      if (!state.activeSessionId) {
        const selectedProject = state.workspace?.projects.find(
          (project) => project.selected,
        );
        if (selectedProject) {
          const selectedSession = selectedProject.sessions.find(
            (session) => session.selected,
          );
          if (selectedSession) {
            await selectSession(selectedProject, selectedSession);
          } else if (projectSessions(selectedProject).length === 0) {
            await newSession(selectedProject);
          }
        }
      }
    } catch {
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
  return { addLocalProject, toggleProject, reorderProjects, removeProject };
}

export { mergeImportedProject, createProjects };
