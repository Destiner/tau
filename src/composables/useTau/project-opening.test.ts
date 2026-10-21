import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createController,
  presentRemoteConnectionError,
  state,
  type ProjectSummary,
  type SessionController,
  type SessionSummary,
  type WorkspaceSnapshot,
} from '../state';

import useTau from './index';

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => vi.fn()),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(async () => null),
}));

function session(
  id: string,
  options: Partial<SessionSummary> = {},
): SessionSummary {
  return {
    id,
    path: `/tmp/${id}.jsonl`,
    title: id,
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: 0,
    archived: false,
    selected: false,
    ...options,
  };
}

function project(
  path: string,
  sessions: SessionSummary[] = [],
  options: Partial<ProjectSummary> = {},
): ProjectSummary {
  return {
    path,
    name: path.split('/').pop() ?? path,
    workingDirectory: path,
    collapsed: false,
    selected: false,
    sessions,
    ...options,
  };
}

function workspace(projects: ProjectSummary[]): WorkspaceSnapshot {
  return {
    activeProjectPath: projects[0]?.path ?? '',
    piPath: '/bin/pi',
    projects,
  };
}

function resetState(projects: ProjectSummary[] = []): void {
  state.workspace = workspace(projects);
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.activeControllerKey = '';
  state.workspaceFeedback = [];
  state.removingProjectPaths = [];
  state.remoteDialogOpen = false;
  state.remoteDialogMode = 'add';
  state.remoteDialogStep = 'connection';
  state.remoteConnectionString = '';
  state.remoteConnectionError = '';
  state.remoteConnecting = false;
  state.remoteDirectoryHost = '';
  state.remoteWorkingDirectory = '';
  state.remoteDirectoryHistory = [];
  state.remoteDirectories = [];
}

async function expectActive(
  projectPath: string,
  sessionId: string,
): Promise<void> {
  await vi.waitFor(() => {
    expect(state.activeProjectPath).toBe(projectPath);
    expect(state.activeSessionId).toBe(sessionId);
  });
}

let tau: ReturnType<typeof useTau> | undefined;

beforeEach(() => {
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command) => {
      if (command === 'start_pi' || command === 'start_pi_remote') return 1;
      return undefined;
    });
  vi.mocked(open).mockReset().mockResolvedValue(null);
  resetState();
});

afterEach(() => {
  tau?.dispose();
  tau = undefined;
});

describe('project opening', () => {
  it('opens a local project from another active project and restores its controller and draft on return', async () => {
    const sourceSession = session('source', { selected: true });
    const source = project('/projects/source', [sourceSession]);
    const existingSession = session('existing', { selected: true });
    const existing = project('/projects/existing', [existingSession], {
      collapsed: true,
    });
    resetState([source, existing]);
    vi.mocked(open).mockResolvedValue(existing.path);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return { ...existing, selected: true };
      if (command === 'start_pi') return 1;
      return undefined;
    });

    tau = useTau();
    await tau.selectSession(source, sourceSession);
    const controller = tau.activeController.value;
    if (!controller) throw new Error('Expected the source controller');
    controller.ready = true;
    controller.generation = 1;
    tau.draft.value = 'Keep this project draft';
    controller.messages.push({
      id: 'preserved',
      kind: 'assistant',
      text: 'Keep this controller',
    });

    await tau.addLocalProject();
    await expectActive(existing.path, existingSession.id);
    expect(existing.collapsed).toBe(false);
    const startsBeforeReturn = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === 'start_pi').length;

    await tau.selectSession(source, sourceSession);
    await expectActive(source.path, sourceSession.id);
    expect(tau.activeController.value).toBe(controller);
    expect(tau.draft.value).toBe('Keep this project draft');
    expect(controller.messages).toEqual([
      expect.objectContaining({ text: 'Keep this controller' }),
    ]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === 'start_pi'),
    ).toHaveLength(startsBeforeReturn);
  });

  it('opens a remote project from another active project and restores its controller and draft on return', async () => {
    const sourceSession = session('source', { selected: true });
    const source = project('/projects/source', [sourceSession]);
    const selected = session('selected', { selected: true });
    const existing = project('ssh:example:/work', [selected], {
      connectionString: 'dev@example',
    });
    resetState([source, existing]);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_remote_project')
        return { ...existing, selected: true };
      if (command === 'start_pi' || command === 'start_pi_remote') return 1;
      return undefined;
    });
    tau = useTau();
    await tau.selectSession(source, sourceSession);
    const controller = tau.activeController.value;
    if (!controller) throw new Error('Expected the source controller');
    controller.ready = true;
    controller.generation = 1;
    tau.draft.value = 'Keep local draft';
    tau.openRemoteProjectDialog();
    state.remoteDialogStep = 'directory';
    state.remoteConnectionString = 'dev@example';
    state.remoteDirectoryHost = 'example';
    state.remoteWorkingDirectory = '/work';

    await tau.chooseRemoteDirectory('/work', 'select');
    await expectActive(existing.path, selected.id);
    expect(state.remoteDialogOpen).toBe(false);

    await tau.selectSession(source, sourceSession);
    expect(tau.activeController.value).toBe(controller);
    expect(tau.draft.value).toBe('Keep local draft');
  });

  it('uses the active, selected, recent non-archived, and empty-project fallbacks', async () => {
    const active = session('active');
    const selected = session('selected', { selected: true, sortAt: 2 });
    const recent = session('recent', { sortAt: 3 });
    const archived = session('archived', { archived: true, sortAt: 4 });
    const populated = project('/projects/fallbacks', [
      active,
      selected,
      recent,
      archived,
    ]);
    const empty = project('/projects/empty');
    const archivedOnly = project('/projects/archived-only', [
      session('only-archived', { archived: true, selected: true }),
    ]);
    resetState([populated, empty, archivedOnly]);
    tau = useTau();

    state.activeProjectPath = populated.path;
    state.activeSessionId = active.id;
    tau.activateProject(populated);
    await expectActive(populated.path, active.id);

    state.activeProjectPath = '';
    state.activeSessionId = '';
    tau.activateProject(populated);
    await expectActive(populated.path, selected.id);

    selected.selected = false;
    state.activeProjectPath = '';
    state.activeSessionId = '';
    tau.activateProject(populated);
    await expectActive(populated.path, recent.id);

    tau.activateProject(empty);
    await vi.waitFor(() => {
      expect(state.activeProjectPath).toBe(empty.path);
      expect(state.activeSessionId).toMatch(/^phantom-/);
    });

    tau.activateProject(archivedOnly);
    await vi.waitFor(() => {
      expect(state.activeProjectPath).toBe(archivedOnly.path);
      expect(state.activeSessionId).toMatch(/^phantom-/);
    });
  });

  it('opens a first remote project into a new session', async () => {
    const remote = project('ssh:example:/empty', [], {
      connectionString: 'dev@example',
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_remote_project') return remote;
      if (command === 'start_pi_remote') return 1;
      return undefined;
    });
    tau = useTau();
    tau.openRemoteProjectDialog();
    state.remoteDialogStep = 'directory';
    state.remoteConnectionString = 'dev@example';
    state.remoteDirectoryHost = 'example';
    state.remoteWorkingDirectory = '/empty';

    await tau.chooseRemoteDirectory('/empty', 'select');

    await vi.waitFor(() => {
      expect(state.activeProjectPath).toBe(remote.path);
      expect(state.activeSessionId).toMatch(/^phantom-/);
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      'start_pi_remote',
      expect.anything(),
    );
  });

  it('keeps a draft-bearing ephemeral session eligible when reopening its project', async () => {
    const sourceSession = session('source', { selected: true });
    const source = project('/projects/source', [sourceSession]);
    const retained = project('/projects/retained');
    resetState([source, retained]);
    vi.mocked(open).mockResolvedValue(retained.path);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return retained;
      if (command === 'start_pi') return 1;
      return undefined;
    });
    tau = useTau();

    const ephemeral = await tau.newSession(retained);
    if (!ephemeral) throw new Error('Expected an ephemeral controller');
    tau.draft.value = 'Continue this workflow';
    await tau.selectSession(source, sourceSession);
    await tau.addLocalProject();

    await expectActive(retained.path, ephemeral.sessionId);
    expect(tau.activeController.value).toBe(ephemeral);
    expect(tau.draft.value).toBe('Continue this workflow');
  });

  it('waits for a deferred Pi-path refresh and does not restart on same-session reselection', async () => {
    const existingSession = session('existing', { selected: true });
    const existing = project('/projects/existing', [existingSession]);
    resetState([existing]);
    state.workspace!.piPath = null;
    const refreshed = deferred<WorkspaceSnapshot>();
    vi.mocked(open).mockResolvedValue(existing.path);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return existing;
      if (command === 'load_workspace') return refreshed.promise;
      if (command === 'start_pi') return 1;
      return undefined;
    });
    tau = useTau();

    const opening = tau.addLocalProject();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith('load_workspace', expect.anything()),
    );
    expect(state.activeProjectPath).toBe('');
    refreshed.resolve(workspace([existing]));
    await opening;
    await expectActive(existing.path, existingSession.id);
    const controller = tau.activeController.value;
    if (!controller) throw new Error('Expected the existing controller');
    expect(state.workspace?.piPath).toBe('/bin/pi');
    const starts = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === 'start_pi').length;

    await tau.selectSession(existing, existingSession);

    expect(tau.activeController.value).toBe(controller);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) => command === 'start_pi'),
    ).toHaveLength(starts);
  });

  it('merges imported sessions without replacing local metadata or imported selection', async () => {
    const retained = session('retained', {
      selected: true,
      title: 'Local title',
    });
    const existing = project('/projects/merge', [retained], {
      collapsed: true,
      selected: true,
      name: 'Local name',
    });
    const imported = project(
      '/projects/merge',
      [
        { ...retained, title: 'Imported title', selected: false },
        session('added', { selected: true }),
      ],
      { selected: true, name: 'Imported name' },
    );
    resetState([existing]);
    vi.mocked(open).mockResolvedValue(imported.path);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return imported;
      if (command === 'start_pi') return 1;
      return undefined;
    });
    tau = useTau();

    await tau.addLocalProject();

    expect(state.workspace?.projects).toHaveLength(1);
    expect(existing).toMatchObject({
      name: 'Local name',
      collapsed: false,
      selected: true,
    });
    expect(existing.sessions).toEqual([
      expect.objectContaining({
        id: retained.id,
        title: 'Local title',
        selected: true,
      }),
      expect.objectContaining({ id: 'added', selected: false }),
    ]);
  });

  it('leaves state unchanged for a cancelled or failed local picker/import', async () => {
    const existing = project('/projects/kept', [
      session('kept', { selected: true }),
    ]);
    resetState([existing]);
    tau = useTau();

    await tau.addLocalProject();
    expect(state.workspace?.projects).toEqual([existing]);

    vi.mocked(open).mockRejectedValueOnce(new Error('picker failed'));
    await tau.addLocalProject();
    expect(state.workspaceFeedback.at(-1)?.message).toContain('folder picker');

    vi.mocked(open).mockResolvedValue('/projects/failing');
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') throw new Error('import failed');
      return undefined;
    });
    await tau.addLocalProject();
    expect(state.workspace?.projects).toEqual([existing]);
    expect(state.workspaceFeedback.at(-1)?.message).toContain(
      'could not be added',
    );

    tau.openRemoteProjectDialog();
    state.remoteDialogStep = 'directory';
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_remote_project') throw new Error('import failed');
      return undefined;
    });
    await tau.chooseRemoteDirectory('/remote', 'select');
    expect(state.remoteDialogOpen).toBe(true);
    expect(state.remoteConnectionError).toContain('could not be added');
  });

  it('does not let a deferred local chooser or import override later navigation', async () => {
    const first = project('/projects/first', [
      session('first', { selected: true }),
    ]);
    const second = project('/projects/second', [
      session('second', { selected: true }),
    ]);
    resetState([first, second]);
    const firstChooser = deferred<string | null>();
    const secondChooser = deferred<string | null>();
    const imported = deferred<ProjectSummary>();
    vi.mocked(open)
      .mockReturnValueOnce(firstChooser.promise)
      .mockReturnValueOnce(secondChooser.promise);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return imported.promise;
      if (command === 'start_pi') return 1;
      return undefined;
    });
    tau = useTau();

    const opening = tau.addLocalProject();
    await tau.selectSession(first, first.sessions[0]!);
    firstChooser.resolve(second.path);
    await opening;
    expect(invoke).not.toHaveBeenCalledWith(
      'import_project',
      expect.anything(),
    );

    const secondOpening = tau.addLocalProject();
    secondChooser.resolve(second.path);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        'import_project',
        expect.objectContaining({ path: second.path }),
      ),
    );
    await tau.newSession(first);
    imported.resolve(second);
    await secondOpening;
    expect(state.workspace?.projects).toEqual([first, second]);
    expect(state.activeProjectPath).toBe(first.path);
  });

  it('allows a later local chooser while an older import is pending, without letting the older import activate', async () => {
    const first = project('/projects/first', [
      session('first', { selected: true }),
    ]);
    const imported = project('/projects/imported', [
      session('imported', { selected: true }),
    ]);
    resetState([first]);
    const olderImport = deferred<ProjectSummary>();
    vi.mocked(open)
      .mockResolvedValueOnce(imported.path)
      .mockResolvedValueOnce(null);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_project') return olderImport.promise;
      if (command === 'start_pi') return 1;
      return undefined;
    });
    tau = useTau();

    const olderOpening = tau.addLocalProject();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        'import_project',
        expect.objectContaining({ path: imported.path }),
      ),
    );

    const newerOpening = tau.addLocalProject();
    await newerOpening;
    olderImport.resolve(imported);
    await olderOpening;

    expect(vi.mocked(open)).toHaveBeenCalledTimes(2);
    expect(state.workspace?.projects).toEqual([first]);
    expect(state.activeProjectPath).toBe('');
  });

  it('invalidates a deferred remote import on session, new-session, navigation, and remote-dialog changes', async () => {
    const first = project('/projects/first', [
      session('first', { selected: true }),
    ]);
    const remote = project(
      'ssh:host:/work',
      [session('remote', { selected: true })],
      {
        connectionString: 'me@host',
      },
    );
    resetState([first]);
    const imports = [
      deferred<ProjectSummary>(),
      deferred<ProjectSummary>(),
      deferred<ProjectSummary>(),
      deferred<ProjectSummary>(),
    ];
    let importIndex = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_remote_project')
        return imports[importIndex++]!.promise;
      if (command === 'start_pi' || command === 'start_pi_remote') return 1;
      return undefined;
    });
    tau = useTau();

    for (const [index, supersede] of [
      (): Promise<void> => tau!.selectSession(first, first.sessions[0]!),
      (): Promise<SessionController | undefined> => tau!.newSession(first),
      (): void => tau!.activateProject(first),
      (): void => tau!.openRemoteProjectDialog(),
    ].entries()) {
      tau.openRemoteProjectDialog();
      state.remoteDialogStep = 'directory';
      state.remoteConnectionString = 'me@host';
      const choosing = tau.chooseRemoteDirectory('/work', 'select');
      await vi.waitFor(() => expect(importIndex).toBe(index + 1));
      supersede();
      imports[index]!.resolve(remote);
      await choosing;
      expect(state.workspace?.projects).toEqual([first]);
    }
  });

  it('does not let a deferred remote import add a project after its source is removed', async () => {
    const source = project('/projects/source', [
      session('source', { selected: true }),
    ]);
    const remote = project('ssh:host:/work', [], {
      connectionString: 'me@host',
    });
    resetState([source]);
    const imported = deferred<ProjectSummary>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'import_remote_project') return imported.promise;
      return undefined;
    });
    tau = useTau();
    tau.openRemoteProjectDialog();
    state.remoteDialogStep = 'directory';
    state.remoteConnectionString = 'me@host';
    const choosing = tau.chooseRemoteDirectory('/work', 'select');
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        'import_remote_project',
        expect.anything(),
      ),
    );

    tau.requestProjectRemoval(source);
    const removing = tau.confirmProjectRemoval();
    imported.resolve(remote);
    await Promise.all([choosing, removing]);

    expect(state.workspace?.projects).toEqual([]);
    expect(state.activeProjectPath).toBe('');
  });

  it('does not let a replaced remote directory listing own the replacement dialog error', async () => {
    const listing = deferred<never>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_remote_directories') return listing.promise;
      return undefined;
    });
    tau = useTau();
    tau.openRemoteProjectDialog();
    state.remoteDialogStep = 'directory';
    state.remoteConnectionString = 'me@host';
    state.remoteWorkingDirectory = '/work';

    const browsing = tau.chooseRemoteDirectory('/next', 'forward');
    tau.openRemoteProjectDialog();
    listing.reject(new Error('listing failed'));
    await browsing;

    expect(state.remoteDialogOpen).toBe(true);
    expect(state.remoteDialogStep).toBe('connection');
    expect(state.remoteConnectionError).toBe('');
  });

  it.each(['probe', 'listing', 'import'] as const)(
    'keeps a runtime-created retry dialog from stale add %s cleanup',
    async (operation) => {
      const remote = project('ssh:stable:/work', [session('remote')], {
        connectionString: 'stable@remote',
      });
      resetState([remote]);
      const pending = deferred<
        | ProjectSummary
        | {
            connectionString: string;
            workingDirectory: string;
            host: string;
            directories: [];
          }
      >();
      vi.mocked(invoke).mockImplementation(async (command) => {
        if (
          command === 'probe_remote_project' ||
          command === 'list_remote_directories' ||
          command === 'import_remote_project'
        ) {
          return pending.promise;
        }
        return undefined;
      });
      tau = useTau();
      const controller = createController(
        remote,
        remote.sessions[0]!,
        operation,
      );
      state.activeControllerKey = controller.key;

      tau.openRemoteProjectDialog();
      state.remoteConnectionString = 'add@host';
      let request: Promise<void>;
      if (operation === 'probe') {
        request = tau.submitRemoteConnection();
      } else {
        state.remoteDialogStep = 'directory';
        state.remoteDirectoryHost = 'add-host';
        state.remoteWorkingDirectory = '/add';
        request = tau.chooseRemoteDirectory(
          operation === 'listing' ? '/next' : '/add',
          operation === 'listing' ? 'forward' : 'select',
        );
      }
      await vi.waitFor(() => expect(state.remoteConnecting).toBe(true));

      presentRemoteConnectionError(controller, 'Runtime retry');
      pending.resolve(
        operation === 'import'
          ? remote
          : {
              connectionString: 'stale@host',
              workingDirectory: '/stale',
              host: 'stale-host',
              directories: [],
            },
      );
      await request;

      expect(state.remoteDialogMode).toBe('retry');
      expect(state.remoteDialogOpen).toBe(true);
      expect(state.remoteDialogStep).toBe('connection');
      expect(state.remoteConnectionString).toBe('stable@remote');
      expect(state.remoteConnectionError).toBe('Runtime retry');
      expect(state.remoteConnecting).toBe(false);
      expect(state.remoteWorkingDirectory).toBe('');
      expect(state.remoteDirectories).toEqual([]);
    },
  );

  it('keeps deferred remote dialog responses owned by the dialog and lifecycle that requested them', async () => {
    const probe = deferred<{
      connectionString: string;
      workingDirectory: string;
      host: string;
      directories: [];
    }>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'probe_remote_project') return probe.promise;
      return undefined;
    });
    tau = useTau();
    tau.openRemoteProjectDialog();
    state.remoteConnectionString = 'me@host';
    const submitting = tau.submitRemoteConnection();
    // Connecting dialogs cannot be closed, but a replacement dialog must
    // still prevent the old probe from taking ownership of its state.
    tau.openRemoteProjectDialog();
    probe.resolve({
      connectionString: 'me@host',
      workingDirectory: '/work',
      host: 'host',
      directories: [],
    });
    await submitting;
    expect(state.remoteDialogOpen).toBe(true);
    expect(state.remoteDialogStep).toBe('connection');

    const chooser = deferred<string | null>();
    vi.mocked(open).mockReturnValue(chooser.promise);
    const localOpening = tau.addLocalProject();
    tau.dispose();
    chooser.resolve('/projects/late');
    await localOpening;
    expect(invoke).not.toHaveBeenCalledWith(
      'import_project',
      expect.anything(),
    );
  });
});
