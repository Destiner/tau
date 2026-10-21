import { projectActionsDisabled, state, type ProjectSummary } from '../state';

import { currentLifecycle } from './navigation';
import { writeWorkspace } from './workspace-writes';

type Rename = NonNullable<typeof state.projectRename>;
const drafts = new Map<string, Rename>();
const operations = new Map<
  string,
  { identity: symbol; timer: ReturnType<typeof setTimeout> }
>();

function clearProjectRename(): void {
  for (const operation of operations.values()) clearTimeout(operation.timer);
  operations.clear();
  drafts.clear();
  state.projectRename = null;
}

function requestProjectRename(project: ProjectSummary): void {
  if (projectActionsDisabled.value) return;
  const target = state.workspace?.projects.find(
    (item) => item.path === project.path,
  );
  if (!target) return;
  state.projectRename = drafts.get(target.path) ?? {
    path: target.path,
    draft: target.name,
    error: '',
    busy: false,
    timedOut: false,
  };
  drafts.set(target.path, state.projectRename);
}

function cancelProjectRename(): void {
  const rename = state.projectRename;
  if (!rename || (rename.busy && !rename.timedOut)) return;
  state.projectRename = null;
}

async function confirmProjectRename(): Promise<void> {
  const rename = state.projectRename;
  const workspace = state.workspace;
  if (!rename || rename.busy || !workspace || operations.has(rename.path))
    return;
  const project = workspace.projects.find((item) => item.path === rename.path);
  if (!project) {
    state.projectRename = null;
    return;
  }
  const name = rename.draft.trim();
  if (!name) {
    rename.error = 'Enter a project name.';
    return;
  }
  if (name.length > 240) {
    rename.error = 'Use 240 characters or fewer.';
    return;
  }
  if (name === project.name) {
    drafts.delete(rename.path);
    state.projectRename = null;
    return;
  }

  const confirmed = project.name;
  const lifecycle = currentLifecycle();
  const identity = Symbol();
  const current = (): boolean =>
    operations.get(rename.path)?.identity === identity &&
    currentLifecycle() === lifecycle &&
    state.workspace === workspace &&
    workspace.projects.includes(project);
  rename.busy = true;
  rename.timedOut = false;
  rename.error = '';
  project.name = name;
  const timer = setTimeout(() => {
    if (!current()) return;
    rename.timedOut = true;
    rename.error =
      'The rename has not been confirmed. You can close this dialog while saving finishes.';
  }, 10_000);
  operations.set(rename.path, { identity, timer });

  try {
    const savedName = await writeWorkspace<string>('rename_project', {
      path: project.path,
      name,
    });
    if (!current()) return;
    project.name = savedName;
    if (drafts.get(rename.path) === rename) drafts.delete(rename.path);
    if (state.projectRename === rename) state.projectRename = null;
  } catch {
    if (!current()) return;
    project.name = confirmed;
    rename.error = 'The project could not be renamed. Try again.';
  } finally {
    clearTimeout(timer);
    if (operations.get(rename.path)?.identity === identity) {
      operations.delete(rename.path);
      rename.busy = false;
      rename.timedOut = false;
    }
  }
}

export {
  requestProjectRename,
  cancelProjectRename,
  confirmProjectRename,
  clearProjectRename,
};
