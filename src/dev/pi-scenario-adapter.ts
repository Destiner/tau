import type { InvokeArgs } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { nextTick } from 'vue';

import {
  PiScenarioEngine,
  type PiScenarioGateState,
  type PiScenarioMetadata,
  type PiScenarioTimelineEntry,
  type ResolvedPiOutput,
} from '../../tests/support/pi-scenario';
import {
  findPiScenario,
  piScenarioCatalogue,
} from '../../tests/support/pi-scenario/catalogue';
import { state, type WorkspaceSnapshot } from '../composables/state';
import type { PiBridgeEvent } from '../lib/pi/bridge';

interface ScenarioVerification {
  ok: boolean;
  error?: string;
  timeline: readonly PiScenarioTimelineEntry[];
}

interface BrowserPiScenarioApi {
  scenario(): PiScenarioMetadata;
  verify(): ScenarioVerification;
  timeline(): readonly PiScenarioTimelineEntry[];
  nativeInvocationCount(command: string): number;
  hasRegisteredSession(sessionId: string): boolean;
  gates(): readonly PiScenarioGateState[];
  waitForGate(name: string): Promise<void>;
  releaseGate(name: string): Promise<void>;
}

interface StartPiArgs {
  runtimeId: string;
  projectPath: string;
  sessionPath: string | null;
}

interface StartPiRemoteArgs {
  runtimeId: string;
  connectionString: string;
  workingDirectory: string;
  sessionPath: string | null;
}

interface SendPiArgs {
  runtimeId: string;
  request: Record<string, unknown>;
}

interface RegisterSessionArgs {
  projectPath: string;
  sessionId: string;
  sessionPath: string;
  sessionName: string;
  adopted: boolean;
}

interface SetActiveSessionArgs {
  projectPath: string;
  sessionId: string;
}

interface NativeSessionIdentity {
  id: string;
  path: string;
  name: string;
  adopted?: boolean;
}

declare global {
  interface Window {
    __TAU_PI_SCENARIO__?: BrowserPiScenarioApi;
  }
}

const PROJECT_PATH = '/fixture/tau-project';
const REMOTE_CONNECTION = 'ssh fixture@example';
const REMOTE_WORKING_DIRECTORY = '/remote/tau-project';
const SESSION_ID = 'session-main';
const SESSION_PATH = `${PROJECT_PATH}/session-main.jsonl`;
const MAIN_SESSION = { id: SESSION_ID, path: SESSION_PATH, name: 'Main' };
const BACKUP_SESSION = {
  id: 'session-backup',
  path: `${PROJECT_PATH}/session-backup.jsonl`,
  name: 'Backup',
};
const REPLACEMENT_SESSION = {
  id: 'session-plan-42',
  path: `${PROJECT_PATH}/session-plan-42.jsonl`,
  name: '42 • plan',
};
const PLAN_SESSION = {
  id: 'session-plan',
  path: `${PROJECT_PATH}/session-plan.jsonl`,
  name: 'docs · RHI-6267 · Plan',
};
const IMPLEMENT_SESSION = {
  id: 'session-implement',
  path: `${PROJECT_PATH}/session-implement.jsonl`,
  name: 'docs · RHI-6267 · Implement',
};
const COMMAND_SESSION = {
  id: 'session-mcp',
  path: `${PROJECT_PATH}/session-mcp.jsonl`,
  name: 'MCP workflow',
};
const FIRST_PROMPT_SESSION = {
  id: 'session-first-prompt',
  path: `${PROJECT_PATH}/session-first-prompt.jsonl`,
  name: 'First prompt',
};
const ARCHIVED_SESSION = {
  id: 'session-archived',
  path: `${PROJECT_PATH}/session-archived.jsonl`,
  name: 'Older archived work',
};
const REQUIRED_NATIVE_COUNTS = {
  'empty-workspace': {
    load_workspace: 1,
    'plugin:dialog|open': 1,
  },
  'saved-session-bootstrap': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-history': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-long-history': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-short-history': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-extension-prompt': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'empty-session-extension-prompt': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-conversation': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 3,
    set_active_session: 3,
  },
  'saved-session-compaction': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 5,
    set_active_session: 5,
  },
  'saved-session-compaction-notifications': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-prompt-admission': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 5,
    set_active_session: 5,
  },
  'saved-session-stale-generation': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 2,
    set_active_session: 2,
  },
  'saved-session-command-replacement': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 4,
    set_active_project: 1,
    set_active_session: 7,
  },
  'plan-implement-replacement': {
    load_workspace: 1,
    read_model_scope: 3,
    register_session: 5,
    set_active_project: 1,
    set_active_session: 9,
  },
  'phantom-command-registration': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 3,
    set_active_project: 1,
    set_active_session: 3,
  },
  'phantom-first-prompt-registration': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 3,
    set_active_project: 1,
    set_active_session: 3,
  },
  'phantom-command-only': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 1,
    set_active_project: 1,
    set_active_session: 2,
  },
  'archived-sessions-review': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 2,
    set_active_session: 3,
    unarchive_session: 1,
  },
  'saved-session-unacknowledged-abort': {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 3,
    set_active_session: 3,
  },
  'remote-phantom-prompt-process-exit': {
    load_workspace: 1,
    read_remote_model_scope: 3,
    start_pi_remote: 3,
    register_session: 1,
    set_active_project: 1,
    set_active_session: 2,
  },
  'remote-saved-session-process-exit': {
    load_workspace: 1,
    read_remote_model_scope: 2,
    start_pi_remote: 2,
    register_session: 2,
    set_active_session: 2,
  },
  'saved-session-bootstrap-process-exit': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 1,
    set_active_session: 1,
  },
  'saved-session-prompt-process-exit': {
    load_workspace: 1,
    read_model_scope: 2,
    register_session: 3,
    set_active_session: 6,
  },
} as const;

const initialWorkspace: WorkspaceSnapshot = {
  activeProjectPath: PROJECT_PATH,
  piPath: '/fixture/bin/pi',
  projects: [
    {
      path: PROJECT_PATH,
      name: 'Tau fixture',
      workingDirectory: PROJECT_PATH,
      collapsed: false,
      selected: true,
      sessions: [
        {
          id: SESSION_ID,
          path: SESSION_PATH,
          title: 'Main',
          lastActive: '2026-01-02T03:04:05.000Z',
          lastUserMessageAt: 0,
          sortAt: 1,
          archived: false,
          selected: true,
        },
      ],
    },
  ],
};

function scenarioWorkspace(scenarioName: string): WorkspaceSnapshot {
  const workspace = structuredClone(initialWorkspace);
  if (scenarioName === 'empty-workspace') {
    workspace.activeProjectPath = '';
    workspace.projects = [];
    return workspace;
  }
  if (
    scenarioName === 'saved-session-prompt-process-exit' ||
    scenarioName === 'saved-session-command-replacement' ||
    scenarioName === 'plan-implement-replacement'
  ) {
    workspace.projects[0]?.sessions.push({
      id: BACKUP_SESSION.id,
      path: BACKUP_SESSION.path,
      title: BACKUP_SESSION.name,
      lastActive: '2026-01-02T03:04:04.000Z',
      lastUserMessageAt: 0,
      sortAt: 0,
      archived: false,
      selected: false,
    });
  }
  if (
    scenarioName === 'remote-phantom-prompt-process-exit' ||
    scenarioName === 'remote-saved-session-process-exit'
  ) {
    const project = workspace.projects[0];
    if (!project) throw new Error('Remote scenario requires a project.');
    project.connectionString = REMOTE_CONNECTION;
    project.workingDirectory = REMOTE_WORKING_DIRECTORY;
    workspace.piPath = null;
  }
  if (scenarioName === 'archived-sessions-review') {
    workspace.projects[0]?.sessions.push({
      id: ARCHIVED_SESSION.id,
      path: ARCHIVED_SESSION.path,
      title: ARCHIVED_SESSION.name,
      model: 'alpha',
      lastActive: '2026-01-02T03:04:03.000Z',
      lastUserMessageAt: 0,
      // Agent-only activity has no user-message timestamp, but must still
      // appear with its current effective activity time.
      sortAt: Date.now(),
      archived: true,
      selected: false,
    });
  }
  return workspace;
}

function installPiScenarioAdapter(scenarioName: string): void {
  const scenario = findPiScenario(scenarioName);
  const ownershipDelay = Number(
    new URLSearchParams(globalThis.location.search).get('ownership-delay') ?? 0,
  );
  if (!scenario) {
    throw new Error(
      `Unknown browser Pi scenario ${JSON.stringify(scenarioName)}. Available: ${piScenarioCatalogue()
        .map(({ name }) => name)
        .join(', ')}.`,
    );
  }

  const engine = new PiScenarioEngine(scenario);
  const nativeCounts = new Map<string, number>();
  const registeredSessionIds = new Set<string>();
  let workspace = scenarioWorkspace(scenarioName);
  const boundRuntimeIds = new Set<string>();
  const startCounts = new Map<string, number>();
  let ownershipRevision = 0;
  let activeOwner = '';
  let adapterFailure: Error | undefined;

  function count(command: string): number {
    const next = (nativeCounts.get(command) ?? 0) + 1;
    nativeCounts.set(command, next);
    return next;
  }

  function rememberFailure(error: unknown): Error {
    const failure =
      error instanceof Error ? error : new Error('Browser Pi adapter failed.');
    adapterFailure ??= failure;
    return failure;
  }

  async function drainOutputs(): Promise<void> {
    let output: ResolvedPiOutput | undefined;
    while ((output = engine.takeOutput())) {
      await emit<PiBridgeEvent>('pi-event', bridgeEvent(output));
      await yieldToRenderedState();
    }
  }

  async function yieldToRenderedState(): Promise<void> {
    // Event listeners are asynchronous; flush one task and Vue update so each
    // ordered fake output is independently observable without a timer.
    await new Promise<void>((resolve) => {
      const channel = new MessageChannel();
      channel.port1.onmessage = (): void => {
        channel.port1.close();
        channel.port2.close();
        resolve();
      };
      channel.port2.postMessage(null);
    });
    await nextTick();
  }

  async function handleCommand(
    command: string,
    rawArgs?: InvokeArgs,
  ): Promise<unknown> {
    try {
      if (rawArgs !== undefined && !isRecord(rawArgs)) {
        throw new Error(`${command} arguments must be an object.`);
      }
      const args = rawArgs ?? {};
      if (command === 'read_pi_frontend_revision') {
        return ownershipRevision;
      }
      if (command === 'claim_pi_frontend') {
        if (ownershipDelay > 0) {
          await new Promise((resolve) => setTimeout(resolve, ownershipDelay));
        }
        const ownerId = requiredString(args, 'ownerId', command);
        if (activeOwner === ownerId) return null;
        if (args.expectedRevision !== ownershipRevision) {
          throw new Error('claim_pi_frontend received a stale revision.');
        }
        ownershipRevision += 1;
        activeOwner = ownerId;
        boundRuntimeIds.clear();
        count(command);
        return null;
      }
      if (command === 'load_workspace') {
        count(command);
        return structuredClone(workspace);
      }
      if (
        command === 'read_model_scope' ||
        (command === 'read_remote_model_scope' &&
          (scenarioName === 'remote-phantom-prompt-process-exit' ||
            scenarioName === 'remote-saved-session-process-exit'))
      ) {
        count(command);
        return [];
      }
      if (
        command === 'plugin:dialog|open' &&
        scenarioName === 'empty-workspace'
      ) {
        count(command);
        return null;
      }
      if (command === 'start_pi' || command === 'start_pi_remote') {
        requireOwner(args, activeOwner, command);
        const value =
          command === 'start_pi' ? startPiArgs(args) : startPiRemoteArgs(args);
        count(command);
        const runtimeKey = scenarioRuntimeKey(
          scenarioName,
          value.sessionPath,
          startCounts,
        );
        boundRuntimeIds.add(value.runtimeId);
        const generation = engine.bindRuntime(runtimeKey, value.runtimeId);
        await drainOutputs();
        return generation;
      }
      if (command === 'send_pi') {
        requireOwner(args, activeOwner, command);
        const value = sendPiArgs(args);
        count(command);
        requireBoundRuntime(value.runtimeId, boundRuntimeIds, command);
        engine.consumeRequest(value.runtimeId, value.request);
        await drainOutputs();
        return null;
      }
      if (command === 'stop_pi') {
        requireOwner(args, activeOwner, command);
        const runtimeId = requiredString(args, 'runtimeId', command);
        count(command);
        requireBoundRuntime(runtimeId, boundRuntimeIds, command);
        return null;
      }
      if (command === 'register_session') {
        const invocation = count(command);
        const expected =
          scenarioName === 'phantom-command-registration' ||
          scenarioName === 'phantom-first-prompt-registration'
            ? ((): NativeSessionIdentity => {
                const session =
                  [MAIN_SESSION, COMMAND_SESSION, FIRST_PROMPT_SESSION].find(
                    (candidate) => candidate.id === args.sessionId,
                  ) ?? MAIN_SESSION;
                return {
                  ...session,
                  adopted:
                    session.id !== MAIN_SESSION.id &&
                    !registeredSessionIds.has(session.id),
                };
              })()
            : expectedNativeSession(scenarioName, command, invocation);
        const value = registerSessionArgs(args, expected);
        registeredSessionIds.add(value.sessionId);
        if (expected.id === REPLACEMENT_SESSION.id) {
          workspace = replacementWorkspace();
        } else if (expected.id === PLAN_SESSION.id) {
          workspace = upsertSessionWorkspace(workspace, PLAN_SESSION);
        } else if (expected.id === COMMAND_SESSION.id) {
          workspace = commandSessionWorkspace();
        } else if (expected.id === FIRST_PROMPT_SESSION.id) {
          workspace = upsertSessionWorkspace(workspace, FIRST_PROMPT_SESSION);
        }
        return structuredClone(workspace);
      }
      if (command === 'set_active_project') {
        count(command);
        requireEqual(
          requiredString(args, 'path', command),
          PROJECT_PATH,
          'set_active_project.path',
        );
        selectWorkspaceSession(workspace, '');
        return structuredClone(workspace);
      }
      if (command === 'set_active_session') {
        const invocation = count(command);
        let expected: NativeSessionIdentity;
        if (
          scenarioName === 'phantom-command-registration' ||
          scenarioName === 'phantom-first-prompt-registration'
        ) {
          expected =
            [MAIN_SESSION, COMMAND_SESSION, FIRST_PROMPT_SESSION].find(
              (session) => session.id === args.sessionId,
            ) ?? MAIN_SESSION;
        } else if (
          scenarioName === 'saved-session-command-replacement' ||
          scenarioName === 'plan-implement-replacement'
        ) {
          const requested = [
            MAIN_SESSION,
            BACKUP_SESSION,
            REPLACEMENT_SESSION,
            PLAN_SESSION,
            IMPLEMENT_SESSION,
          ].find((session) => session.id === args.sessionId);
          if (!requested) {
            throw new Error('set_active_session used an unknown session.');
          }
          expected = requested;
        } else {
          expected = expectedNativeSession(scenarioName, command, invocation);
        }
        const value = setActiveSessionArgs(args, expected);
        const registered = workspace.projects
          .find((project) => project.path === value.projectPath)
          ?.sessions.some((session) => session.id === value.sessionId);
        if (!registered) {
          throw new Error('set_active_session used an unknown session.');
        }
        selectWorkspaceSession(workspace, expected.id);
        return structuredClone(workspace);
      }
      if (command === 'unarchive_session') {
        count(command);
        const sessionId = requiredString(args, 'sessionId', command);
        const session = workspace.projects
          .flatMap((project) => project.sessions)
          .find((candidate) => candidate.id === sessionId);
        if (!session) {
          throw new Error(`Unarchived unknown session ${sessionId}.`);
        }
        session.archived = false;
        return structuredClone(workspace);
      }
      // Admin mode has no scenario state: a scenario always starts in the
      // default off, and a test that types the cheat code drives the
      // frontend switch it is actually exercising.
      if (command === 'read_admin_mode') {
        count(command);
        return false;
      }
      if (command === 'pending_quit_request') {
        count(command);
        return null;
      }
      if (command === 'set_admin_mode') {
        if (typeof args.enabled !== 'boolean') {
          throw new Error('set_admin_mode enabled must be a boolean.');
        }
        count(command);
        return null;
      }
      if (command === 'ingest_telemetry') {
        if (!Array.isArray(args.records)) {
          throw new Error('ingest_telemetry records must be an array.');
        }
        count(command);
        return null;
      }
      if (command === 'plugin:window|show') {
        if (args.label !== 'main') {
          throw new Error('Window show must target the main window.');
        }
        count(command);
        return null;
      }
      throw new Error(`Unexpected Tauri command ${JSON.stringify(command)}.`);
    } catch (error) {
      throw rememberFailure(error);
    }
  }

  function verification(): ScenarioVerification {
    try {
      if (adapterFailure) throw adapterFailure;
      engine.verifyComplete();
      for (const [command, expected] of Object.entries(
        REQUIRED_NATIVE_COUNTS[
          scenarioName as keyof typeof REQUIRED_NATIVE_COUNTS
        ],
      )) {
        const actual = nativeCounts.get(command) ?? 0;
        if (actual !== expected) {
          throw new Error(
            `${command} invocation count: expected ${expected}, received ${actual}.`,
          );
        }
      }
      return { ok: true, timeline: engine.timeline() };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : 'Scenario failed.',
        timeline: engine.timeline(),
      };
    }
  }

  mockWindows('main');
  mockIPC(handleCommand, { shouldMockEvents: true });
  window.__TAU_PI_SCENARIO__ = {
    scenario: (): PiScenarioMetadata => structuredClone(scenario.metadata),
    verify: verification,
    timeline: (): readonly PiScenarioTimelineEntry[] => engine.timeline(),
    nativeInvocationCount: (command: string): number =>
      nativeCounts.get(command) ?? 0,
    hasRegisteredSession: (sessionId: string): boolean =>
      Boolean(
        state.workspace?.projects.some((project) =>
          project.sessions.some((session) => session.id === sessionId),
        ),
      ) && !state.ephemeralSessions.some((session) => session.id === sessionId),
    gates: (): readonly PiScenarioGateState[] => engine.gates(),
    waitForGate: async (name: string): Promise<void> => {
      try {
        await engine.waitForGateReached(name);
      } catch (error) {
        throw rememberFailure(error);
      }
    },
    releaseGate: async (name: string): Promise<void> => {
      try {
        engine.releaseGate(name);
        await drainOutputs();
      } catch (error) {
        throw rememberFailure(error);
      }
    },
  };
}

function bridgeEvent(output: ResolvedPiOutput): PiBridgeEvent {
  const base = {
    runtimeId: output.runtime.id,
    generation: output.runtime.generation,
  };
  if (output.kind === 'runtime-event') {
    return { ...base, ...output.value };
  }
  return { ...base, kind: 'rpc', line: JSON.stringify(output.value) };
}

function startPiArgs(args: Record<string, unknown>): StartPiArgs {
  const sessionPath = args.sessionPath;
  if (sessionPath !== null && typeof sessionPath !== 'string') {
    throw new Error('start_pi.sessionPath must be a string or null.');
  }
  const value = {
    runtimeId: requiredString(args, 'runtimeId', 'start_pi'),
    projectPath: requiredString(args, 'projectPath', 'start_pi'),
    sessionPath,
  };
  requireEqual(value.projectPath, PROJECT_PATH, 'start_pi.projectPath');
  if (
    value.sessionPath !== null &&
    value.sessionPath !== SESSION_PATH &&
    value.sessionPath !== BACKUP_SESSION.path &&
    value.sessionPath !== ARCHIVED_SESSION.path &&
    value.sessionPath !== PLAN_SESSION.path
  ) {
    throw new Error('start_pi.sessionPath must identify a fixture session.');
  }
  return value;
}

function startPiRemoteArgs(args: Record<string, unknown>): StartPiRemoteArgs {
  const sessionPath = args.sessionPath;
  if (sessionPath !== null && typeof sessionPath !== 'string') {
    throw new Error('start_pi_remote.sessionPath must be a string or null.');
  }
  const value = {
    runtimeId: requiredString(args, 'runtimeId', 'start_pi_remote'),
    connectionString: requiredString(
      args,
      'connectionString',
      'start_pi_remote',
    ),
    workingDirectory: requiredString(
      args,
      'workingDirectory',
      'start_pi_remote',
    ),
    sessionPath,
  };
  requireEqual(
    value.connectionString,
    REMOTE_CONNECTION,
    'start_pi_remote.connectionString',
  );
  requireEqual(
    value.workingDirectory,
    REMOTE_WORKING_DIRECTORY,
    'start_pi_remote.workingDirectory',
  );
  return value;
}

function sendPiArgs(args: Record<string, unknown>): SendPiArgs {
  const runtimeId = requiredString(args, 'runtimeId', 'send_pi');
  const request = args.request;
  if (!isRecord(request)) throw new Error('send_pi.request must be an object.');
  return { runtimeId, request };
}

function registerSessionArgs(
  args: Record<string, unknown>,
  expected: NativeSessionIdentity,
): RegisterSessionArgs {
  const adopted = args.adopted;
  if (typeof adopted !== 'boolean') {
    throw new Error('register_session.adopted must be a boolean.');
  }
  const value = {
    projectPath: requiredString(args, 'projectPath', 'register_session'),
    sessionId: requiredString(args, 'sessionId', 'register_session'),
    sessionPath: requiredString(args, 'sessionPath', 'register_session'),
    sessionName: requiredString(args, 'sessionName', 'register_session'),
    adopted,
  };
  requireEqual(value.projectPath, PROJECT_PATH, 'register_session.projectPath');
  requireEqual(value.sessionId, expected.id, 'register_session.sessionId');
  requireEqual(
    value.sessionPath,
    expected.path,
    'register_session.sessionPath',
  );
  requireEqual(
    value.sessionName,
    expected.name,
    'register_session.sessionName',
  );
  // A session Pi hands over must be registered as adopted, or Tau cannot show
  // a phase session again once its row has been archived. Opening an already
  // archived session is the exception: browsing must not resurrect it.
  requireEqual(
    String(value.adopted),
    String(
      expected.adopted ??
        (expected.id !== SESSION_ID &&
          expected.id !== BACKUP_SESSION.id &&
          expected.id !== ARCHIVED_SESSION.id),
    ),
    'register_session.adopted',
  );
  return value;
}

function setActiveSessionArgs(
  args: Record<string, unknown>,
  expected: NativeSessionIdentity,
): SetActiveSessionArgs {
  const value = {
    projectPath: requiredString(args, 'projectPath', 'set_active_session'),
    sessionId: requiredString(args, 'sessionId', 'set_active_session'),
  };
  requireEqual(
    value.projectPath,
    PROJECT_PATH,
    'set_active_session.projectPath',
  );
  requireEqual(value.sessionId, expected.id, 'set_active_session.sessionId');
  return value;
}

function expectedNativeSession(
  scenarioName: string,
  command: 'register_session' | 'set_active_session',
  invocation: number,
): NativeSessionIdentity {
  if (scenarioName === 'saved-session-command-replacement') {
    const sequence =
      command === 'register_session'
        ? [
            MAIN_SESSION,
            BACKUP_SESSION,
            REPLACEMENT_SESSION,
            { ...REPLACEMENT_SESSION, adopted: false },
          ]
        : [
            MAIN_SESSION,
            BACKUP_SESSION,
            MAIN_SESSION,
            BACKUP_SESSION,
            REPLACEMENT_SESSION,
          ];
    const expected = sequence[invocation - 1];
    if (!expected) throw new Error(`${command} ran too many times.`);
    return expected;
  }
  if (scenarioName === 'plan-implement-replacement') {
    const sequence =
      command === 'register_session'
        ? [
            MAIN_SESSION,
            BACKUP_SESSION,
            PLAN_SESSION,
            { ...PLAN_SESSION, adopted: false },
            { ...PLAN_SESSION, adopted: false },
          ]
        : [
            MAIN_SESSION,
            BACKUP_SESSION,
            BACKUP_SESSION,
            MAIN_SESSION,
            BACKUP_SESSION,
            PLAN_SESSION,
            PLAN_SESSION,
          ];
    const expected = sequence[invocation - 1];
    if (!expected) throw new Error(`${command} ran too many times.`);
    return expected;
  }
  if (scenarioName === 'saved-session-prompt-process-exit') {
    const sequence =
      command === 'register_session'
        ? [MAIN_SESSION, BACKUP_SESSION, MAIN_SESSION]
        : [
            MAIN_SESSION,
            BACKUP_SESSION,
            BACKUP_SESSION,
            MAIN_SESSION,
            MAIN_SESSION,
            BACKUP_SESSION,
          ];
    const expected = sequence[invocation - 1];
    if (!expected) throw new Error(`${command} ran too many times.`);
    return expected;
  }
  if (scenarioName === 'archived-sessions-review') {
    // Opening the archived session selects its record without adopting it.
    return invocation >= 2 ? ARCHIVED_SESSION : MAIN_SESSION;
  }
  return MAIN_SESSION;
}

function scenarioRuntimeKey(
  scenarioName: string,
  sessionPath: string | null,
  startCounts: Map<string, number>,
): string {
  const pathKey = sessionPath ?? '<new-session>';
  const count = (startCounts.get(pathKey) ?? 0) + 1;
  startCounts.set(pathKey, count);
  if (scenarioName === 'remote-phantom-prompt-process-exit') {
    if (sessionPath === SESSION_PATH) return 'main';
    if (sessionPath !== null || count > 2) {
      throw new Error(
        'Remote phantom recovery used an unexpected runtime start.',
      );
    }
    return count === 1 ? 'failed-phantom' : 'recovered-phantom';
  }
  if (scenarioName === 'remote-saved-session-process-exit') {
    if (sessionPath !== SESSION_PATH || count > 2) {
      throw new Error(
        'Remote saved-session recovery used an unexpected runtime start.',
      );
    }
    return count === 1 ? 'main' : 'reconnected-main';
  }
  if (scenarioName === 'saved-session-bootstrap-process-exit') {
    if (sessionPath !== SESSION_PATH || count > 2) {
      throw new Error('Bootstrap recovery used an unexpected runtime start.');
    }
    return count === 1 ? 'failed-bootstrap' : 'recovered-main';
  }
  if (
    scenarioName === 'saved-session-prompt-process-exit' ||
    scenarioName === 'saved-session-command-replacement' ||
    scenarioName === 'plan-implement-replacement'
  ) {
    if (count > 1) throw new Error('A scenario session started twice.');
    if (sessionPath === BACKUP_SESSION.path) return 'backup';
    if (
      scenarioName === 'plan-implement-replacement' &&
      sessionPath === PLAN_SESSION.path
    ) {
      return 'reopened-plan';
    }
    return scenarioName === 'plan-implement-replacement' ? 'workflow' : 'main';
  }
  if (
    scenarioName === 'phantom-command-registration' ||
    scenarioName === 'phantom-first-prompt-registration' ||
    scenarioName === 'phantom-command-only'
  ) {
    if (count > 1) throw new Error('A command session started twice.');
    return sessionPath === null ? 'phantom' : 'main';
  }
  if (count > 1) throw new Error('start_pi may only run once per session.');
  if (
    scenarioName === 'archived-sessions-review' &&
    sessionPath === '/fixture/tau-project/session-archived.jsonl'
  ) {
    return 'archived';
  }
  return 'main';
}

function selectWorkspaceSession(
  workspace: WorkspaceSnapshot,
  sessionId: string,
): void {
  for (const project of workspace.projects) {
    project.selected = project.path === PROJECT_PATH;
    for (const session of project.sessions) {
      session.selected = session.id === sessionId;
    }
  }
  workspace.activeProjectPath = PROJECT_PATH;
}

function commandSessionWorkspace(): WorkspaceSnapshot {
  const workspace = structuredClone(initialWorkspace);
  const project = workspace.projects[0];
  const oldSession = project?.sessions[0];
  if (!project || !oldSession) {
    throw new Error('Command fixture requires the initial saved session.');
  }
  oldSession.selected = false;
  project.sessions.push({
    id: COMMAND_SESSION.id,
    path: COMMAND_SESSION.path,
    title: COMMAND_SESSION.name,
    lastActive: '2026-01-02T03:04:06.000Z',
    lastUserMessageAt: 0,
    sortAt: 2,
    archived: false,
    selected: true,
  });
  return workspace;
}

function upsertSessionWorkspace(
  source: WorkspaceSnapshot,
  identity: NativeSessionIdentity,
): WorkspaceSnapshot {
  const workspace = structuredClone(source);
  const project = workspace.projects[0];
  if (!project) throw new Error('Session fixture requires a project.');
  const existing = project.sessions.find(
    (session) => session.id === identity.id,
  );
  if (existing) {
    existing.path = identity.path;
    existing.title = identity.name;
    return workspace;
  }
  for (const session of project.sessions) session.selected = false;
  project.sessions.push({
    id: identity.id,
    path: identity.path,
    title: identity.name,
    lastActive: '2026-01-02T03:04:06.000Z',
    lastUserMessageAt: 0,
    sortAt: 2,
    archived: false,
    selected: true,
  });
  return workspace;
}

function replacementWorkspace(): WorkspaceSnapshot {
  const workspace = structuredClone(initialWorkspace);
  const project = workspace.projects[0];
  const oldSession = project?.sessions[0];
  if (!project || !oldSession) {
    throw new Error('Replacement fixture requires the initial saved session.');
  }
  oldSession.selected = false;
  project.sessions.push({
    id: REPLACEMENT_SESSION.id,
    path: REPLACEMENT_SESSION.path,
    title: REPLACEMENT_SESSION.name,
    lastActive: '2026-01-02T03:04:06.000Z',
    lastUserMessageAt: 0,
    sortAt: 2,
    archived: false,
    selected: true,
  });
  return workspace;
}

function requiredString(
  args: Record<string, unknown>,
  field: string,
  command: string,
): string {
  const value = args[field];
  if (typeof value !== 'string' || !value) {
    throw new Error(`${command}.${field} must be a non-empty string.`);
  }
  return value;
}

function requireOwner(
  args: Record<string, unknown>,
  activeOwner: string,
  command: string,
): void {
  if (!activeOwner || args.ownerId !== activeOwner) {
    throw new Error(`${command} rejected a stale Pi frontend owner.`);
  }
}

function requireEqual(actual: string, expected: string, label: string): void {
  if (actual !== expected) {
    throw new Error(
      `${label}: expected ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}.`,
    );
  }
}

function requireBoundRuntime(
  runtimeId: string,
  boundRuntimeIds: ReadonlySet<string>,
  command: string,
): void {
  if (!boundRuntimeIds.has(runtimeId)) {
    throw new Error(`${command} used an unbound runtime.`);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export default installPiScenarioAdapter;
