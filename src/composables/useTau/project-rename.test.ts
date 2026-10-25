import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createController,
  setActiveSessionView,
  state,
  type ProjectSummary,
} from '../state';

import useTau from './index';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => vi.fn()),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: () => void;
} {
  let resolve!: (value: T) => void;
  let reject!: () => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = (): void => fail(new Error('write failed'));
  });
  return { promise, resolve, reject };
}

function fixture(path: string, name = 'Original'): ProjectSummary {
  return {
    path,
    name,
    workingDirectory: path,
    collapsed: true,
    selected: false,
    sessions: [
      {
        id: 'archived',
        path: `${path}/session.jsonl`,
        title: 'Saved',
        lastActive: 'now',
        lastUserMessageAt: 1,
        sortAt: 1,
        archived: true,
        selected: false,
      },
    ],
  };
}

let tau: ReturnType<typeof useTau>;
let project: ProjectSummary;
let other: ProjectSummary;

beforeEach(() => {
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command, args) =>
      command === 'rename_project'
        ? (args as { name: string }).name
        : undefined,
    );
  state.workspace = {
    activeProjectPath: '/one',
    piPath: '/bin/pi',
    projects: [fixture('/one'), fixture('/two')],
  };
  project = state.workspace.projects[0]!;
  other = state.workspace.projects[1]!;
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.removingProjectPaths = [];
  state.remoteDialogOpen = false;
  state.projectRemoval = null;
  state.workspaceFeedback = [];
  state.initializing = false;
  tau = useTau();
});

afterEach(() => {
  tau.dispose();
  vi.useRealTimers();
});

function request(name = 'Renamed'): void {
  tau.openProjectRename(project);
  if (!state.projectRename) throw new Error('Expected rename dialog');
  state.projectRename.name = name;
}

async function flushWrite(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('project rename', () => {
  it('delays progress by 300 ms and clears both timers on fast success', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValueOnce(write.promise);
    request();
    const saving = tau.submitProjectRename();
    expect(state.projectRename?.showProgress).toBe(false);
    await vi.advanceTimersByTimeAsync(299);
    expect(state.projectRename?.showProgress).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(state.projectRename?.showProgress).toBe(true);
    write.resolve('Renamed');
    await saving;
    expect(vi.getTimerCount()).toBe(0);
    request('Fast');
    await tau.submitProjectRename();
    expect(state.projectRename).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('validates UTF-16 length rather than Unicode code points', async () => {
    request('🦀'.repeat(120) + 'x');
    await tau.submitProjectRename();
    expect(state.projectRename?.error).toBe('Use 240 characters or fewer.');
    expect(invoke).not.toHaveBeenCalled();
    state.projectRename!.name = `  ${'🦀'.repeat(120)}  `;
    await tau.submitProjectRename();
    expect(project.name).toBe('🦀'.repeat(120));
  });

  it.each(['success', 'failure'] as const)(
    'late %s cannot close a reopened same-project dialog or erase its edited draft',
    async (outcome) => {
      vi.useFakeTimers();
      const write = deferred<string>();
      vi.mocked(invoke).mockReturnValueOnce(write.promise);
      request('Submitted');
      const saving = tau.submitProjectRename();
      await vi.advanceTimersByTimeAsync(10_000);
      tau.closeProjectRename();
      tau.openProjectRename(project);
      state.projectRename!.name = 'New intent';
      await tau.submitProjectRename();
      expect(invoke).toHaveBeenCalledTimes(1);
      if (outcome === 'success') write.resolve('Confirmed');
      else write.reject();
      await saving;
      expect(state.projectRename).toMatchObject({
        projectPath: project.path,
        name: 'New intent',
        saving: false,
        unconfirmed: false,
        showProgress: false,
      });
      tau.closeProjectRename();
      tau.openProjectRename(project);
      expect(state.projectRename?.name).toBe('New intent');
      await tau.submitProjectRename();
      expect(project.name).toBe('New intent');
    },
  );

  it('does not close a reopened dialog even if its input has not changed', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValueOnce(write.promise);
    request();
    const saving = tau.submitProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.closeProjectRename();
    tau.openProjectRename(project);
    write.resolve('Renamed');
    await saving;
    expect(state.projectRename).toMatchObject({
      name: 'Renamed',
      saving: false,
    });
  });

  it.each(['success', 'failure'] as const)(
    'reconciles rename %s when a subsequent reorder fails',
    async (outcome) => {
      vi.useFakeTimers();
      const write = deferred<string>();
      vi.mocked(invoke).mockImplementation(async (command) => {
        if (command === 'rename_project') return write.promise;
        throw new Error('reorder failed');
      });
      request();
      const saving = tau.submitProjectRename();
      await vi.advanceTimersByTimeAsync(10_000);
      tau.closeProjectRename();
      const reordering = tau.reorderProjects(0, 1);
      expect(state.workspace?.projects).toEqual([other, project]);
      expect(invoke).toHaveBeenCalledTimes(1);
      if (outcome === 'success') write.resolve('Confirmed');
      else write.reject();
      await Promise.all([saving, reordering]);
      expect(state.workspace?.projects).toEqual([project, other]);
      expect(project.name).toBe(
        outcome === 'success' ? 'Confirmed' : 'Original',
      );
      expect(invoke).toHaveBeenNthCalledWith(
        2,
        'reorder_projects',
        expect.objectContaining({ projectPaths: [other.path, project.path] }),
      );
    },
  );

  it('forgets removed targets and their timers without affecting a replacement at the same path', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValueOnce(write.promise);
    request('Removed draft');
    const saving = tau.submitProjectRename();
    await flushWrite();
    state.workspace!.projects = [other];
    expect(state.projectRename).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    state.workspace!.projects.push(fixture(project.path, 'Replacement'));
    const replacement = state.workspace!.projects[1]!;
    tau.openProjectRename(replacement);
    expect(state.projectRename?.name).toBe('Replacement');
    state.projectRename!.name = 'Replacement draft';
    write.resolve('Stale');
    await saving;
    expect(replacement.name).toBe('Replacement');
    expect(state.projectRename?.name).toBe('Replacement draft');
    await tau.submitProjectRename();
    expect(replacement.name).toBe('Replacement draft');
  });

  it('serializes independent project renames without the first result disturbing the second dialog', async () => {
    vi.useFakeTimers();
    const first = deferred<string>();
    const second = deferred<string>();
    vi.mocked(invoke)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    request('First');
    const firstSaving = tau.submitProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.closeProjectRename();
    tau.openProjectRename(other);
    state.projectRename!.name = 'Second';
    const secondSaving = tau.submitProjectRename();
    expect(other.name).toBe('Second');
    expect(invoke).toHaveBeenCalledTimes(1);
    first.resolve('First confirmed');
    await firstSaving;
    await flushWrite();
    expect(project.name).toBe('First confirmed');
    expect(state.projectRename).toMatchObject({
      projectPath: other.path,
      name: 'Second',
      saving: true,
    });
    expect(invoke).toHaveBeenCalledTimes(2);
    second.reject();
    await secondSaving;
    expect(other.name).toBe('Original');
    expect(state.projectRename).toMatchObject({
      projectPath: other.path,
      name: 'Second',
      saving: false,
      error: 'The project could not be renamed. Try again.',
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it('drops cancelled drafts when the project is removed', () => {
    request('Abandoned');
    tau.closeProjectRename();
    state.workspace!.projects = [other];
    state.workspace!.projects.push(fixture(project.path, 'Reimported'));
    tau.openProjectRename(state.workspace!.projects[1]!);
    expect(state.projectRename?.name).toBe('Reimported');
  });

  it('prefills, blocks other project actions, and retains path-keyed drafts on cancel', () => {
    tau.openProjectRename(project);
    expect(state.projectRename).toMatchObject({
      projectPath: project.path,
      name: 'Original',
      saving: false,
      unconfirmed: false,
    });
    expect(tau.projectActionsDisabled.value).toBe(true);
    state.projectRename!.name = 'Unfinished';
    tau.requestProjectRemoval(other);
    expect(state.projectRemoval).toBeNull();
    tau.closeProjectRename();
    expect(tau.projectActionsDisabled.value).toBe(false);
    tau.openProjectRename(other);
    expect(state.projectRename?.name).toBe('Original');
    tau.closeProjectRename();
    tau.openProjectRename(project);
    expect(state.projectRename?.name).toBe('Unfinished');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('validates blank names and closes normalized unchanged names without writing', async () => {
    request('  ');
    await tau.submitProjectRename();
    expect(state.projectRename?.error).toBe('Enter a project name.');
    state.projectRename!.name = '  Original  ';
    await tau.submitProjectRename();
    expect(state.projectRename).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('updates names optimistically, saves trimmed Unicode and punctuation, and clears the successful draft', async () => {
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValue(write.promise);
    request('  Новый / project 🦀  ');
    const saving = tau.submitProjectRename();
    expect(project.name).toBe('Новый / project 🦀');
    expect(tau.archivedSessionEntries.value[0]).toMatchObject({
      projectPath: project.path,
      projectName: project.name,
    });
    expect(state.projectRename?.saving).toBe(true);
    tau.closeProjectRename();
    expect(state.projectRename).not.toBeNull();
    await tau.submitProjectRename();
    await flushWrite();
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      'rename_project',
      expect.objectContaining({ path: project.path, name: project.name }),
    );
    write.resolve('Confirmed');
    await saving;
    expect(project.name).toBe('Confirmed');
    expect(state.projectRename).toBeNull();
    tau.openProjectRename(project);
    expect(state.projectRename?.name).toBe('Confirmed');
  });

  it('rolls back confirmed failure locally, retaining input for retry', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('failure'));
    request('Retry me');
    await tau.submitProjectRename();
    expect(project.name).toBe('Original');
    expect(state.projectRename).toMatchObject({
      name: 'Retry me',
      saving: false,
      error: 'The project could not be renamed. Try again.',
    });
    expect(state.workspaceFeedback).toEqual([]);
    await tau.submitProjectRename();
    expect(project.name).toBe('Retry me');
    expect(state.projectRename).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it.each(['local', 'remote'] as const)(
    'mutates only the %s project name, preserving runtime, session, draft, archive and workspace identities',
    async (kind) => {
      if (kind === 'remote') {
        project.connectionString = 'user@host';
        project.workingDirectory = '/repo';
        project.path =
          'ssh:{"connectionString":"user@host","workingDirectory":"/repo"}';
      }
      const workspace = state.workspace!;
      const projects = workspace.projects;
      const sessions = project.sessions;
      const archived = sessions[0]!;
      const controller = createController(project, archived, 'live');
      state.controllers.push(controller);
      setActiveSessionView(project, archived, controller);
      controller.generation = 7;
      controller.draft = 'Keep draft';
      controller.messages.push({
        id: 'message',
        kind: 'assistant',
        text: 'Keep text',
      });
      const messages = controller.messages;
      const before = JSON.parse(JSON.stringify(project)) as ProjectSummary;
      const selection = [
        state.activeProjectPath,
        state.activeSessionId,
        state.activeSessionPath,
        state.activeControllerKey,
      ];
      request();
      await tau.submitProjectRename();
      expect(state.workspace).toBe(workspace);
      expect(workspace.projects).toBe(projects);
      expect(project.sessions).toBe(sessions);
      expect(project.sessions[0]).toBe(archived);
      expect(project).toEqual({ ...before, name: 'Renamed' });
      expect(tau.activeController.value).toBe(controller);
      expect(controller.generation).toBe(7);
      expect(controller.draft).toBe('Keep draft');
      expect(controller.messages).toBe(messages);
      expect([
        state.activeProjectPath,
        state.activeSessionId,
        state.activeSessionPath,
        state.activeControllerKey,
      ]).toEqual(selection);
      expect(tau.archivedSessionEntries.value[0]?.session).toBe(archived);
      expect(invoke).toHaveBeenCalledExactlyOnceWith(
        'rename_project',
        expect.objectContaining({ path: project.path, name: 'Renamed' }),
      );
    },
  );

  it.each(['success', 'failure'] as const)(
    'permits dismissal after the deadline but prevents retry until late %s settles',
    async (outcome) => {
      vi.useFakeTimers();
      const write = deferred<string>();
      vi.mocked(invoke).mockReturnValueOnce(write.promise);
      request();
      const saving = tau.submitProjectRename();
      await flushWrite();
      await vi.advanceTimersByTimeAsync(9_999);
      expect(state.projectRename?.unconfirmed).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      expect(state.projectRename).toMatchObject({
        saving: true,
        unconfirmed: true,
        name: 'Renamed',
      });
      expect(state.projectRename?.error).toContain('not been confirmed');
      expect(project.name).toBe('Renamed');
      tau.closeProjectRename();
      expect(state.projectRename).toBeNull();
      tau.openProjectRename(project);
      expect(state.projectRename).toMatchObject({
        saving: true,
        unconfirmed: true,
      });
      await tau.submitProjectRename();
      expect(invoke).toHaveBeenCalledTimes(1);
      tau.closeProjectRename();
      if (outcome === 'success') write.resolve('Renamed');
      else write.reject();
      await saving;
      expect(state.projectRename).toBeNull();
      expect(project.name).toBe(outcome === 'success' ? 'Renamed' : 'Original');
      tau.openProjectRename(project);
      expect(state.projectRename).toMatchObject({
        name: 'Renamed',
        saving: false,
        unconfirmed: false,
      });
      if (outcome === 'failure') {
        expect(state.projectRename?.error).toContain('Try again');
        await tau.submitProjectRename();
        expect(project.name).toBe('Renamed');
        expect(invoke).toHaveBeenCalledTimes(2);
      }
    },
  );

  it('a late result updates its captured target without closing a newer target dialog', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValueOnce(write.promise);
    request();
    const saving = tau.submitProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.closeProjectRename();
    tau.openProjectRename(other);
    state.projectRename!.name = 'Other draft';
    write.resolve('Confirmed');
    await saving;
    expect(project.name).toBe('Confirmed');
    expect(other.name).toBe('Original');
    expect(state.projectRename).toMatchObject({
      projectPath: other.path,
      name: 'Other draft',
    });
  });

  it.each(['removed', 'replaced project', 'disposed'] as const)(
    'ignores late success and failure after target is %s',
    async (change) => {
      for (const outcome of ['success', 'failure']) {
        const write = deferred<string>();
        vi.mocked(invoke).mockReturnValueOnce(write.promise);
        request(`Optimistic ${outcome}`);
        const saving = tau.submitProjectRename();
        await flushWrite();
        const workspace = state.workspace!;
        if (change === 'removed') workspace.projects = [other];
        if (change === 'replaced project')
          workspace.projects = [fixture(project.path, 'Replacement'), other];
        if (change === 'disposed') tau.dispose();
        if (outcome === 'success') write.resolve('Late confirmed');
        else write.reject();
        await saving;
        expect(project.name).toBe(`Optimistic ${outcome}`);
        if (change === 'replaced project')
          expect(workspace.projects[0]?.name).toBe('Replacement');
        expect(workspace.projects).not.toContainEqual(
          expect.objectContaining({ name: 'Late confirmed' }),
        );
        tau.dispose();
        state.workspace = { ...workspace, projects: [project, other] };
      }
    },
  );

  it('reconciles after a reorder replaces the workspace but preserves the captured project', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === 'rename_project' ? write.promise : undefined,
    );
    const workspace = state.workspace;
    request();
    const saving = tau.submitProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.closeProjectRename();
    const reordering = tau.reorderProjects(0, 1);
    expect(state.workspace).not.toBe(workspace);
    expect(state.workspace?.projects).toEqual([other, project]);
    await flushWrite();
    expect(invoke).toHaveBeenCalledTimes(1);
    write.resolve('Confirmed');
    await Promise.all([saving, reordering]);
    expect(project.name).toBe('Confirmed');
    expect(invoke).toHaveBeenNthCalledWith(
      2,
      'reorder_projects',
      expect.objectContaining({ projectPaths: [other.path, project.path] }),
    );
  });

  it('cleans deadline timers and drafts on dispose, and fences a previous operation from a new one', async () => {
    vi.useFakeTimers();
    const first = deferred<string>();
    const second = deferred<string>();
    vi.mocked(invoke)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    request('First');
    const oldSaving = tau.submitProjectRename();
    await flushWrite();
    tau.dispose();
    expect(vi.getTimerCount()).toBe(0);
    expect(state.projectRename).toBeNull();
    tau.openProjectRename(project);
    expect(state.projectRename?.name).toBe('First');
    state.projectRename!.name = 'Second';
    const newSaving = tau.submitProjectRename();
    first.resolve('Stale');
    await oldSaving;
    expect(project.name).toBe('Second');
    expect(state.projectRename).toMatchObject({ name: 'Second', saving: true });
    second.resolve('Second');
    await newSaving;
    expect(project.name).toBe('Second');
    expect(state.projectRename).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });
});
