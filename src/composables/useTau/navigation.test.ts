import { invoke } from '@tauri-apps/api/core';
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
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeControllerKey = '';
  state.removingProjectPaths = [];
});

describe('saved preview', () => {
  it('previews saved content without waiting for runtime readiness', async () => {
    const { project, session } = fixture();
    const saved = deferred<{ messages: unknown[] | null }>();
    const started = deferred<number>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'read_saved_transcript') return saved.promise;
      if (command === 'start_pi') return started.promise;
      return undefined;
    });
    const tau = useTau();
    const selecting = tau.selectSession(project, session);
    expect(state.activeSessionId).toBe(session.id);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        'read_saved_transcript',
        expect.objectContaining({
          sessionId: session.id,
          sessionPath: session.path,
        }),
      ),
    );
    saved.resolve({
      messages: [
        { role: 'user', content: [{ type: 'text', text: 'Saved text' }] },
      ],
    });
    await vi.waitFor(() =>
      expect(tau.messages.value.some((row) => row.text === 'Saved text')).toBe(
        true,
      ),
    );
    expect(tau.activeController.value?.ready).toBe(false);
    started.resolve(1);
    await selecting;
    tau.dispose();
  });

  it('shows a confirmed-empty saved transcript without unlocking the runtime', async () => {
    const { project, session } = fixture();
    const started = deferred<number>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'read_saved_transcript') return { messages: [] };
      if (command === 'start_pi') return started.promise;
      return undefined;
    });
    const tau = useTau();
    const selecting = tau.selectSession(project, session);
    await vi.waitFor(() => expect(tau.sessionLoading.value).toBe(false));
    expect(tau.messages.value).toEqual([]);
    expect(tau.activeController.value?.ready).toBe(false);
    expect(tau.canCompose.value).toBe(false);
    started.resolve(1);
    await selecting;
    tau.dispose();
  });

  it('discards a saved read that resolves after navigating to another session', async () => {
    const { project, session } = fixture();
    const next: SessionSummary = {
      ...session,
      id: 'next',
      path: '/tmp/next.jsonl',
      selected: false,
    };
    project.sessions.push(next);
    const oldRead = deferred<{ messages: unknown[] | null }>();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'read_saved_transcript') {
        return (args as { sessionId: string }).sessionId === session.id
          ? oldRead.promise
          : { messages: null };
      }
      if (command === 'start_pi') return 1;
      return undefined;
    });
    const tau = useTau();
    await tau.selectSession(project, session);
    const first = tau.activeController.value;
    await tau.selectSession(project, next);
    oldRead.resolve({
      messages: [{ role: 'user', content: [{ type: 'text', text: 'Stale' }] }],
    });
    await oldRead.promise;
    await Promise.resolve();
    expect(first?.messages.some((row) => row.text === 'Stale')).toBe(false);
    expect(tau.messages.value.some((row) => row.text === 'Stale')).toBe(false);
    tau.dispose();
  });
});
