import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { PiBridgeEvent } from '../lib/pi/bridge';

import type {
  ProjectSummary,
  SessionSummary,
  WorkspaceSnapshot,
} from './state';
import useTau from './useTau';

/*
 * A workflow extension resumes a phase by switching Pi's runtime onto a
 * session that already exists, which Tau may have archived in the meantime.
 * The native double below mirrors `storage.rs`: registration upserts the row
 * and clears `archived` only when Tau adopts the session Pi handed it, and
 * selection rejects a missing or archived row. `revives` turns the native
 * revive off, standing in for a registry that keeps the row archived.
 */

interface SessionRecord {
  id: string;
  path: string;
  name: string;
  archived: boolean;
}

const PROJECT_PATH = '/tmp/archived-adoption';
const LIVE_SESSION = { id: 'live', path: '/tmp/live.jsonl', name: 'live' };
const PHASE_SESSION = {
  id: 'monitor-phase',
  path: '/tmp/monitor-phase.jsonl',
  name: 'infra · RHI-6034 · Monitor',
};
const UNAVAILABLE = 'The selected session is not available in Tau.';

const mocks = vi.hoisted(() => ({
  records: [] as Array<{
    id: string;
    path: string;
    name: string;
    archived: boolean;
  }>,
  activeSessionId: '',
  generation: 0,
  revives: true,
  listener: undefined as
    ((event: { payload: PiBridgeEvent }) => void) | undefined,
}));

function snapshot(): WorkspaceSnapshot {
  return {
    activeProjectPath: PROJECT_PATH,
    piPath: '/usr/local/bin/pi',
    projects: [
      {
        path: PROJECT_PATH,
        name: 'archived-adoption',
        workingDirectory: PROJECT_PATH,
        collapsed: false,
        selected: true,
        sessions: mocks.records.map((record) => ({
          id: record.id,
          path: record.path,
          title: record.name,
          lastActive: 'now',
          lastUserMessageAt: 0,
          sortAt: 1,
          archived: record.archived,
          selected: !record.archived && record.id === mocks.activeSessionId,
        })),
      },
    ],
  };
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command.startsWith('start_pi')) {
      mocks.generation += 1;
      return mocks.generation;
    }
    if (command.endsWith('model_scope')) return [];
    if (command === 'register_session') {
      const id = args?.sessionId as string;
      const name = (args?.sessionName as string | null) ?? '';
      const record = mocks.records.find((entry) => entry.id === id);
      if (!record) {
        mocks.records.push({
          id,
          path: args?.sessionPath as string,
          name: name || 'New Session',
          archived: false,
        });
        return snapshot();
      }
      record.path = args?.sessionPath as string;
      if (name) record.name = name;
      if (args?.adopted === true && mocks.revives) record.archived = false;
      return snapshot();
    }
    if (command === 'set_active_session') {
      const id = args?.sessionId as string;
      const record = mocks.records.find((entry) => entry.id === id);
      if (!record || record.archived) throw UNAVAILABLE;
      mocks.activeSessionId = id;
      return snapshot();
    }
    return snapshot();
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(
    async (
      _event: string,
      listener: (event: { payload: PiBridgeEvent }) => void,
    ) => {
      mocks.listener = listener;
      return vi.fn();
    },
  ),
}));

function emitRpc(
  controller: { runtimeId: string; generation: number },
  value: Record<string, unknown>,
): void {
  mocks.listener?.({
    payload: {
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'rpc',
      line: JSON.stringify(value),
    },
  });
}

function sentRequests(
  controller: { runtimeId: string },
  type: string,
): Array<Record<string, unknown>> {
  return vi
    .mocked(invoke)
    .mock.calls.filter(
      ([command, args]) =>
        command === 'send_pi' &&
        (args as { runtimeId?: string })?.runtimeId === controller.runtimeId,
    )
    .map(([, args]) => (args as { request: Record<string, unknown> }).request)
    .filter((request) => request.type === type);
}

function replacementState(): Record<string, unknown> {
  return {
    model: { provider: 'provider', id: 'alpha', name: 'Alpha' },
    thinkingLevel: 'high',
    sessionId: PHASE_SESSION.id,
    sessionFile: PHASE_SESSION.path,
    sessionName: PHASE_SESSION.name,
    isStreaming: true,
  };
}

function setup(records: SessionRecord[]): {
  tau: ReturnType<typeof useTau>;
  project: ProjectSummary;
} {
  mocks.records = records.map((record) => ({ ...record }));
  mocks.activeSessionId = LIVE_SESSION.id;
  mocks.generation = 0;
  const tau = useTau();
  tau.dispose();
  tau.state.activeProjectPath = '';
  tau.state.activeSessionId = '';
  tau.state.activeSessionPath = '';
  tau.state.activeControllerKey = '';
  tau.state.controllers.splice(0);
  tau.state.ephemeralSessions.splice(0);
  tau.state.workspace = snapshot();
  return { tau, project: tau.state.workspace.projects[0] as ProjectSummary };
}

function visibleSessions(tau: ReturnType<typeof useTau>): SessionSummary[] {
  const project = tau.state.workspace?.projects[0];
  if (!project) throw new Error('Expected the fixture project');
  return tau.projectSessions(project);
}

beforeEach(() => {
  mocks.revives = true;
});

describe('a workflow phase Pi hands back', () => {
  it('brings an archived session back into the sidebar', async () => {
    const { tau, project } = setup([
      { ...LIVE_SESSION, archived: false },
      { ...PHASE_SESSION, archived: true },
    ]);
    await tau.initialize();
    const live = project.sessions.find(
      (session) => session.id === LIVE_SESSION.id,
    ) as SessionSummary;
    await tau.selectSession(project, live);
    const controller = tau.state.controllers.find(
      (candidate) => candidate.sessionId === LIVE_SESSION.id,
    );
    if (!controller) throw new Error('Expected the live controller');
    controller.starting = false;
    controller.ready = true;
    controller.commands = [{ name: 'implement', source: 'extension' }];
    controller.commandsLoaded = true;

    expect(visibleSessions(tau).map((session) => session.id)).toEqual([
      LIVE_SESSION.id,
    ]);

    tau.draft.value = '/implement RHI-6034';
    await tau.sendMessage();
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'prompt')).toHaveLength(1);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'prompt')[0]?.id,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_state').length).toBeGreaterThan(0);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state').at(-1)?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: replacementState(),
    });

    await vi.waitFor(() => {
      expect(tau.state.activeSessionId).toBe(PHASE_SESSION.id);
      expect(visibleSessions(tau).map((session) => session.id)).toContain(
        PHASE_SESSION.id,
      );
    });
    expect(controller.status).toBe('');
    expect(
      mocks.records.find((record) => record.id === PHASE_SESSION.id)?.archived,
    ).toBe(false);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.find(([command]) => command === 'register_session')?.[1],
    ).toMatchObject({ sessionId: PHASE_SESSION.id, adopted: true });
    tau.dispose();
  });

  it('keeps a row for a live session Tau cannot select', async () => {
    mocks.revives = false;
    const { tau, project } = setup([
      { ...LIVE_SESSION, archived: false },
      { ...PHASE_SESSION, archived: true },
    ]);
    await tau.initialize();

    await tau.newSession(project);
    const controller = tau.state.controllers.find(
      (candidate) => candidate.phantom,
    );
    if (!controller) throw new Error('Expected the phantom controller');
    controller.starting = false;
    controller.ready = true;
    controller.commands = [{ name: 'implement', source: 'extension' }];
    controller.commandsLoaded = true;
    controller.currentEffort = 'high';

    tau.draft.value = '/implement RHI-6034';
    await tau.sendMessage();
    // Pi answers with the identity it gave the new session, before the
    // command switches the runtime onto the phase session.
    emitRpc(controller, {
      id: controller.pendingPrompt?.stateRequestId,
      type: 'response',
      command: 'get_state',
      success: true,
      data: {
        ...replacementState(),
        sessionId: 'unwritten',
        sessionFile: '/tmp/unwritten.jsonl',
        sessionName: '',
        isStreaming: false,
      },
    });
    await vi.waitFor(() => {
      expect(controller.pendingPrompt?.messagesRequestId).not.toBe('');
    });
    emitRpc(controller, {
      id: controller.pendingPrompt?.messagesRequestId,
      type: 'response',
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'prompt')).toHaveLength(1);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'prompt')[0]?.id,
      type: 'response',
      command: 'prompt',
      success: true,
    });
    await vi.waitFor(() => {
      expect(sentRequests(controller, 'get_state').length).toBeGreaterThan(1);
    });
    emitRpc(controller, {
      id: sentRequests(controller, 'get_state').at(-1)?.id,
      type: 'response',
      command: 'get_state',
      success: true,
      data: replacementState(),
    });

    await vi.waitFor(() => {
      expect(controller.sessionId).toBe(PHASE_SESSION.id);
      expect(controller.status).toBe(
        'This session could not be saved. Continue here, then try reopening it.',
      );
    });
    // The registry still hides the row from the project list, but the
    // archived list now carries the record, so the session stays reachable
    // without an emergency ephemeral row.
    expect(visibleSessions(tau).map((session) => session.id)).not.toContain(
      PHASE_SESSION.id,
    );
    expect(
      tau.archivedSessionEntries.value.map((entry) => entry.session.id),
    ).toContain(PHASE_SESSION.id);
    tau.dispose();
  });
});
