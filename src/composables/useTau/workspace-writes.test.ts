import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { state, type ProjectSummary, type SessionSummary } from '../state';

import useTau from './index';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => vi.fn()),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function fixture(): { project: ProjectSummary; session: SessionSummary } {
  const session: SessionSummary = {
    id: 'saved',
    path: '/tmp/saved.jsonl',
    title: 'Saved',
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: 0,
    archived: false,
    selected: true,
  };
  const project: ProjectSummary = {
    path: '/tmp/project',
    name: 'project',
    workingDirectory: '/tmp/project',
    collapsed: false,
    selected: true,
    sessions: [session],
  };
  state.workspace = {
    activeProjectPath: project.path,
    piPath: '/bin/pi',
    projects: [project],
  };
  return { project, session };
}

beforeEach(() => {
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async () => undefined);
  vi.mocked(open).mockReset().mockResolvedValue(null);
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeControllerKey = '';
  state.removingProjectPaths = [];
});

describe('workspace persistence and saved preview', () => {
  it('collapses optimistically and persists the latest intent in order', async () => {
    const { project } = fixture();
    const first = deferred<void>();
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === 'set_project_collapsed' &&
      vi.mocked(invoke).mock.calls.filter(([name]) => name === command)
        .length === 1
        ? first.promise
        : undefined,
    );
    const tau = useTau();
    const a = tau.toggleProject(project);
    const b = tau.toggleProject(project);
    expect(project.collapsed).toBe(false);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    first.resolve();
    await Promise.all([a, b]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([name]) => name === 'set_project_collapsed')
        .map(([, args]) => (args as { collapsed: boolean }).collapsed),
    ).toEqual([true, false]);
    tau.dispose();
  });

  it.each(['local', 'remote'] as const)(
    'waits to persist removal until an older %s import has finished',
    async (kind) => {
      const { project } = fixture();
      const imported = deferred<ProjectSummary>();
      vi.mocked(invoke).mockImplementation(async (command) => {
        if (
          command === 'import_project' ||
          command === 'import_remote_project'
        ) {
          return imported.promise;
        }
        return undefined;
      });
      if (kind === 'local') vi.mocked(open).mockResolvedValue('/tmp/imported');
      const tau = useTau();

      let importing: Promise<void>;
      if (kind === 'local') {
        importing = tau.addLocalProject();
      } else {
        tau.openRemoteProjectDialog();
        state.remoteDialogStep = 'directory';
        state.remoteConnectionString = 'me@host';
        state.remoteDirectoryHost = 'host';
        state.remoteWorkingDirectory = '/tmp/imported';
        importing = tau.chooseRemoteDirectory('/tmp/imported', 'select');
      }
      const importCommand =
        kind === 'local' ? 'import_project' : 'import_remote_project';
      await vi.waitFor(() =>
        expect(invoke).toHaveBeenCalledWith(importCommand, expect.anything()),
      );

      tau.requestProjectRemoval(project);
      const removing = tau.confirmProjectRemoval();
      expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
        importCommand,
      ]);

      imported.resolve({
        path: '/tmp/imported',
        name: 'imported',
        workingDirectory: '/tmp/imported',
        collapsed: false,
        selected: false,
        sessions: [],
      });
      await Promise.all([importing, removing]);

      expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
        importCommand,
        'remove_project',
      ]);
      tau.dispose();
    },
  );

  it('archives and restores rows optimistically without replacing the workspace', async () => {
    const { project, session } = fixture();
    const archived = deferred<void>();
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === 'archive_session' ? archived.promise : undefined,
    );
    const tau = useTau();
    const workspace = state.workspace;
    const action = tau.archiveSession(project, session);
    expect(session.archived).toBe(true);
    expect(tau.projectSessions(project)).not.toContain(session);
    archived.resolve();
    await action;
    expect(state.workspace).toBe(workspace);
    const restoring = deferred<void>();
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === 'unarchive_session' ? restoring.promise : undefined,
    );
    const restore = tau.unarchiveSession(project, session);
    expect(tau.projectSessions(project)).toContain(session);
    restoring.resolve();
    await restore;
    tau.dispose();
  });
});
