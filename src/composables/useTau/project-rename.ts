import { reactive, watch } from 'vue';

import { projectActionsDisabled, state, type ProjectSummary } from '../state';

import { currentLifecycle } from './navigation';
import { writeWorkspace } from './workspace-writes';

type Rename = NonNullable<typeof state.projectRename>;
interface Draft {
  project: ProjectSummary;
  form: Rename;
  opening: number;
}
interface Operation {
  timers: ReturnType<typeof setTimeout>[];
}
const drafts = new Map<string, Draft>();
const operations = new Map<string, Operation>();
let opening = 0;
let stopWatching: (() => void) | undefined;

function forgetProjectRename(path: string): void {
  const operation = operations.get(path);
  if (operation) for (const timer of operation.timers) clearTimeout(timer);
  operations.delete(path);
  drafts.delete(path);
  if (state.projectRename?.projectPath === path) state.projectRename = null;
}

function clearProjectRename(): void {
  stopWatching?.();
  stopWatching = undefined;
  for (const path of drafts.keys()) forgetProjectRename(path);
  state.projectRename = null;
}

function watchTargets(): void {
  stopWatching ??= watch(
    () => state.workspace?.projects.slice(),
    (projects) => {
      for (const [path, draft] of drafts) {
        if (!projects?.includes(draft.project)) forgetProjectRename(path);
      }
    },
    { flush: 'sync' },
  );
}

function openProjectRename(project: ProjectSummary): void {
  if (projectActionsDisabled.value || state.remoteDialogOpen) return;
  const target = state.workspace?.projects.find(
    (item) => item.path === project.path,
  );
  if (!target) return;
  watchTargets();
  let draft = drafts.get(target.path);
  if (!draft || draft.project !== target) {
    draft = {
      project: target,
      opening: 0,
      form: reactive({
        projectPath: target.path,
        name: target.name,
        saving: false,
        showProgress: false,
        unconfirmed: false,
        error: '',
      }),
    };
    drafts.set(target.path, draft);
  }
  draft.opening = ++opening;
  state.projectRename = draft.form;
}

function closeProjectRename(): void {
  const form = state.projectRename;
  if (!form || (form.saving && !form.unconfirmed)) return;
  state.projectRename = null;
}

async function submitProjectRename(): Promise<void> {
  const form = state.projectRename;
  if (!form || form.saving || operations.has(form.projectPath)) return;
  const draft = drafts.get(form.projectPath);
  if (!draft || draft.form !== form) return;
  const project = draft.project;
  if (!state.workspace?.projects.includes(project)) {
    forgetProjectRename(form.projectPath);
    return;
  }
  const name = form.name.trim();
  if (!name) {
    form.error = 'Enter a project name.';
    return;
  }
  if (name.length > 240) {
    form.error = 'Use 240 characters or fewer.';
    return;
  }
  if (name === project.name) {
    forgetProjectRename(project.path);
    return;
  }

  const confirmed = project.name;
  const submitted = form.name;
  const dialogOpening = draft.opening;
  const lifecycle = currentLifecycle();
  const operation: Operation = { timers: [] };
  const current = (): boolean =>
    operations.get(project.path) === operation &&
    currentLifecycle() === lifecycle &&
    Boolean(state.workspace?.projects.includes(project));
  operations.set(project.path, operation);
  form.saving = true;
  form.showProgress = false;
  form.unconfirmed = false;
  form.error = '';
  project.name = name;
  operation.timers.push(
    setTimeout(() => {
      if (current()) form.showProgress = true;
    }, 300),
    setTimeout(() => {
      if (!current()) return;
      form.unconfirmed = true;
      form.error =
        'The rename has not been confirmed. You can close this dialog while saving finishes.';
    }, 10_000),
  );

  try {
    const savedName = await writeWorkspace<string>('rename_project', {
      path: project.path,
      name,
    });
    if (!current()) return;
    project.name = savedName;
    // A reopened dialog or edited draft belongs to a newer user intent.
    if (draft.opening === dialogOpening && form.name === submitted) {
      drafts.delete(project.path);
      if (state.projectRename === form) state.projectRename = null;
    } else {
      form.error = '';
    }
  } catch {
    if (!current()) return;
    project.name = confirmed;
    form.error = 'The project could not be renamed. Try again.';
  } finally {
    for (const timer of operation.timers) clearTimeout(timer);
    if (operations.get(project.path) === operation) {
      operations.delete(project.path);
      form.saving = false;
      form.showProgress = false;
      form.unconfirmed = false;
    }
  }
}

export {
  openProjectRename,
  closeProjectRename,
  submitProjectRename,
  clearProjectRename,
  forgetProjectRename,
};
