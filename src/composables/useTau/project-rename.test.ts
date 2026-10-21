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
  tau.requestProjectRename(project);
  if (!state.projectRename) throw new Error('Expected rename dialog');
  state.projectRename.draft = name;
}

async function flushWrite(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('project rename', () => {
  it('prefills, blocks other project actions, and retains path-keyed drafts on cancel', () => {
    tau.requestProjectRename(project);
    expect(state.projectRename).toMatchObject({
      path: project.path,
      draft: 'Original',
      busy: false,
      timedOut: false,
    });
    expect(tau.projectActionsDisabled.value).toBe(true);
    state.projectRename!.draft = 'Unfinished';
    tau.requestProjectRemoval(other);
    expect(state.projectRemoval).toBeNull();
    tau.cancelProjectRename();
    expect(tau.projectActionsDisabled.value).toBe(false);
    tau.requestProjectRename(other);
    expect(state.projectRename?.draft).toBe('Original');
    tau.cancelProjectRename();
    tau.requestProjectRename(project);
    expect(state.projectRename?.draft).toBe('Unfinished');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('validates blank and oversized names and closes normalized unchanged names without writing', async () => {
    request('  ');
    await tau.confirmProjectRename();
    expect(state.projectRename?.error).toBe('Enter a project name.');
    state.projectRename!.draft = 'x'.repeat(241);
    await tau.confirmProjectRename();
    expect(state.projectRename?.error).toBe('Use 240 characters or fewer.');
    state.projectRename!.draft = '  Original  ';
    await tau.confirmProjectRename();
    expect(state.projectRename).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('updates names optimistically, saves trimmed Unicode and punctuation, and clears the successful draft', async () => {
    const write = deferred<string>();
    vi.mocked(invoke).mockReturnValue(write.promise);
    request('  Новый / project 🦀  ');
    const saving = tau.confirmProjectRename();
    expect(project.name).toBe('Новый / project 🦀');
    expect(tau.archivedSessionEntries.value[0]).toMatchObject({
      projectPath: project.path,
      projectName: project.name,
    });
    expect(state.projectRename?.busy).toBe(true);
    tau.cancelProjectRename();
    expect(state.projectRename).not.toBeNull();
    await tau.confirmProjectRename();
    await flushWrite();
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      'rename_project',
      expect.objectContaining({ path: project.path, name: project.name }),
    );
    write.resolve('Confirmed');
    await saving;
    expect(project.name).toBe('Confirmed');
    expect(state.projectRename).toBeNull();
    tau.requestProjectRename(project);
    expect(state.projectRename?.draft).toBe('Confirmed');
  });

  it('rolls back confirmed failure locally, retaining input for retry', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('failure'));
    request('Retry me');
    await tau.confirmProjectRename();
    expect(project.name).toBe('Original');
    expect(state.projectRename).toMatchObject({
      draft: 'Retry me',
      busy: false,
      error: 'The project could not be renamed. Try again.',
    });
    expect(state.workspaceFeedback).toEqual([]);
    await tau.confirmProjectRename();
    expect(project.name).toBe('Retry me');
    expect(state.projectRename).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('accepts duplicate names without selecting or touching the other project', async () => {
    request(other.name);
    state.projectRename!.draft = 'Different';
    other.name = 'Different';
    await tau.confirmProjectRename();
    expect(project.name).toBe(other.name);
    expect(state.workspace?.projects.map((item) => item.path)).toEqual([
      '/one',
      '/two',
    ]);
    expect(project.selected).toBe(false);
    expect(other.selected).toBe(false);
  });

  it('mutates only the name, preserving runtime, session, draft, archive and workspace identities', async () => {
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
    const before = structuredClone(JSON.parse(JSON.stringify(project)));
    const selection = [
      state.activeProjectPath,
      state.activeSessionId,
      state.activeSessionPath,
      state.activeControllerKey,
    ];
    request();
    await tau.confirmProjectRename();
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
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it.each(['success', 'failure'] as const)(
    'permits dismissal after the deadline but prevents retry until late %s settles',
    async (outcome) => {
      vi.useFakeTimers();
      const write = deferred<string>();
      vi.mocked(invoke).mockReturnValueOnce(write.promise);
      request();
      const saving = tau.confirmProjectRename();
      await flushWrite();
      await vi.advanceTimersByTimeAsync(9_999);
      expect(state.projectRename?.timedOut).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      expect(state.projectRename).toMatchObject({
        busy: true,
        timedOut: true,
        draft: 'Renamed',
      });
      expect(state.projectRename?.error).toContain('not been confirmed');
      expect(project.name).toBe('Renamed');
      tau.cancelProjectRename();
      expect(state.projectRename).toBeNull();
      tau.requestProjectRename(project);
      expect(state.projectRename).toMatchObject({ busy: true, timedOut: true });
      await tau.confirmProjectRename();
      expect(invoke).toHaveBeenCalledTimes(1);
      tau.cancelProjectRename();
      if (outcome === 'success') write.resolve('Renamed');
      else write.reject();
      await saving;
      expect(state.projectRename).toBeNull();
      expect(project.name).toBe(outcome === 'success' ? 'Renamed' : 'Original');
      tau.requestProjectRename(project);
      expect(state.projectRename).toMatchObject({
        draft: 'Renamed',
        busy: false,
        timedOut: false,
      });
      if (outcome === 'failure') {
        expect(state.projectRename?.error).toContain('Try again');
        await tau.confirmProjectRename();
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
    const saving = tau.confirmProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.cancelProjectRename();
    tau.requestProjectRename(other);
    state.projectRename!.draft = 'Other draft';
    write.resolve('Confirmed');
    await saving;
    expect(project.name).toBe('Confirmed');
    expect(other.name).toBe('Original');
    expect(state.projectRename).toMatchObject({
      path: other.path,
      draft: 'Other draft',
    });
  });

  it.each([
    'removed',
    'replaced project',
    'replaced workspace',
    'disposed',
  ] as const)(
    'ignores late success and failure after target is %s',
    async (change) => {
      for (const outcome of ['success', 'failure']) {
        const write = deferred<string>();
        vi.mocked(invoke).mockReturnValueOnce(write.promise);
        request(`Optimistic ${outcome}`);
        const saving = tau.confirmProjectRename();
        await flushWrite();
        const workspace = state.workspace!;
        if (change === 'removed') workspace.projects = [other];
        if (change === 'replaced project')
          workspace.projects = [fixture(project.path, 'Replacement'), other];
        if (change === 'replaced workspace') state.workspace = { ...workspace };
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

  it('renames a remote project without changing its encoded identity or starting a connection', async () => {
    const path =
      'ssh:{"connectionString":"user@host","workingDirectory":"/repo"}';
    project.path = path;
    project.connectionString = 'user@host';
    project.workingDirectory = '/repo';
    const before = JSON.parse(JSON.stringify(project)) as ProjectSummary;
    request();
    await tau.confirmProjectRename();
    expect(project).toEqual({ ...before, name: 'Renamed' });
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      'rename_project',
      expect.objectContaining({ path, name: 'Renamed' }),
    );
  });

  it('reconciles after a reorder without replacing the workspace or captured project', async () => {
    vi.useFakeTimers();
    const write = deferred<string>();
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === 'rename_project' ? write.promise : undefined,
    );
    const workspace = state.workspace;
    request();
    const saving = tau.confirmProjectRename();
    await vi.advanceTimersByTimeAsync(10_000);
    tau.cancelProjectRename();
    const reordering = tau.reorderProjects(0, 1);
    expect(state.workspace).toBe(workspace);
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
    const oldSaving = tau.confirmProjectRename();
    await flushWrite();
    tau.dispose();
    expect(vi.getTimerCount()).toBe(0);
    expect(state.projectRename).toBeNull();
    tau.requestProjectRename(project);
    expect(state.projectRename?.draft).toBe('First');
    state.projectRename!.draft = 'Second';
    const newSaving = tau.confirmProjectRename();
    first.resolve('Stale');
    await oldSaving;
    expect(project.name).toBe('Second');
    expect(state.projectRename).toMatchObject({ draft: 'Second', busy: true });
    second.resolve('Second');
    await newSaving;
    expect(project.name).toBe('Second');
    expect(state.projectRename).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });
});
