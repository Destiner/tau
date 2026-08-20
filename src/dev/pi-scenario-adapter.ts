import type { InvokeArgs } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { nextTick } from 'vue';

import {
  PiScenarioEngine,
  type PiScenario,
  type PiScenarioTimelineEntry,
  type ResolvedPiOutput,
} from '../../tests/support/pi-scenario';
import {
  savedSessionBootstrap,
  savedSessionConversation,
} from '../../tests/support/pi-scenario/saved-session-stream-then-stale-generation';
import type { WorkspaceSnapshot } from '../composables/state';
import type { PiBridgeEvent } from '../lib/pi/bridge';

interface ScenarioVerification {
  ok: boolean;
  error?: string;
  timeline: readonly PiScenarioTimelineEntry[];
}

interface BrowserPiScenarioApi {
  verify(): ScenarioVerification;
  timeline(): readonly PiScenarioTimelineEntry[];
}

interface StartPiArgs {
  runtimeId: string;
  projectPath: string;
  sessionPath: string;
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
}

interface SetActiveSessionArgs {
  projectPath: string;
  sessionId: string;
}

declare global {
  interface Window {
    __TAU_PI_SCENARIO__?: BrowserPiScenarioApi;
  }
}

const PROJECT_PATH = '/fixture/tau-project';
const SESSION_ID = 'session-main';
const SESSION_PATH = `${PROJECT_PATH}/session-main.jsonl`;
const SCENARIOS: Readonly<Record<string, PiScenario>> = {
  [savedSessionBootstrap.metadata.name]: savedSessionBootstrap,
  [savedSessionConversation.metadata.name]: savedSessionConversation,
};
const REQUIRED_NATIVE_COUNTS = {
  [savedSessionBootstrap.metadata.name]: {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 1,
    set_active_session: 1,
  },
  [savedSessionConversation.metadata.name]: {
    load_workspace: 1,
    read_model_scope: 1,
    register_session: 3,
    set_active_session: 3,
  },
} as const;

const workspace: WorkspaceSnapshot = {
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

function installPiScenarioAdapter(scenarioName: string): void {
  const scenario = SCENARIOS[scenarioName];
  if (!scenario) {
    throw new Error(
      `Unknown browser Pi scenario ${JSON.stringify(scenarioName)}. Available: ${Object.keys(SCENARIOS).join(', ')}.`,
    );
  }

  const engine = new PiScenarioEngine(scenario);
  const nativeCounts = new Map<string, number>();
  let boundRuntimeId = '';
  let adapterFailure: Error | undefined;

  function count(command: string): void {
    nativeCounts.set(command, (nativeCounts.get(command) ?? 0) + 1);
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
      if (command === 'load_workspace') {
        count(command);
        return structuredClone(workspace);
      }
      if (command === 'read_model_scope') {
        count(command);
        return [];
      }
      if (command === 'start_pi') {
        const value = startPiArgs(args);
        count(command);
        if (boundRuntimeId) throw new Error('start_pi may only run once.');
        boundRuntimeId = value.runtimeId;
        const generation = engine.bindRuntime('main', value.runtimeId);
        await drainOutputs();
        return generation;
      }
      if (command === 'send_pi') {
        const value = sendPiArgs(args);
        count(command);
        requireBoundRuntime(value.runtimeId, boundRuntimeId, command);
        engine.consumeRequest(value.runtimeId, value.request);
        await drainOutputs();
        return null;
      }
      if (command === 'stop_pi') {
        const runtimeId = requiredString(args, 'runtimeId', command);
        count(command);
        requireBoundRuntime(runtimeId, boundRuntimeId, command);
        return null;
      }
      if (command === 'register_session') {
        registerSessionArgs(args);
        count(command);
        return structuredClone(workspace);
      }
      if (command === 'set_active_session') {
        setActiveSessionArgs(args);
        count(command);
        return structuredClone(workspace);
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
    verify: verification,
    timeline: (): readonly PiScenarioTimelineEntry[] => engine.timeline(),
  };
}

function bridgeEvent(output: ResolvedPiOutput): PiBridgeEvent {
  const base = {
    runtimeId: output.runtime.id,
    generation: output.runtime.generation,
  };
  if (output.kind === 'runtime-event') {
    return { ...base, kind: output.value.kind };
  }
  return { ...base, kind: 'rpc', line: JSON.stringify(output.value) };
}

function startPiArgs(args: Record<string, unknown>): StartPiArgs {
  const value = {
    runtimeId: requiredString(args, 'runtimeId', 'start_pi'),
    projectPath: requiredString(args, 'projectPath', 'start_pi'),
    sessionPath: requiredString(args, 'sessionPath', 'start_pi'),
  };
  requireEqual(value.projectPath, PROJECT_PATH, 'start_pi.projectPath');
  requireEqual(value.sessionPath, SESSION_PATH, 'start_pi.sessionPath');
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
): RegisterSessionArgs {
  const value = {
    projectPath: requiredString(args, 'projectPath', 'register_session'),
    sessionId: requiredString(args, 'sessionId', 'register_session'),
    sessionPath: requiredString(args, 'sessionPath', 'register_session'),
    sessionName: requiredString(args, 'sessionName', 'register_session'),
  };
  requireEqual(value.projectPath, PROJECT_PATH, 'register_session.projectPath');
  requireEqual(value.sessionId, SESSION_ID, 'register_session.sessionId');
  requireEqual(value.sessionPath, SESSION_PATH, 'register_session.sessionPath');
  requireEqual(value.sessionName, 'Main', 'register_session.sessionName');
  return value;
}

function setActiveSessionArgs(
  args: Record<string, unknown>,
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
  requireEqual(value.sessionId, SESSION_ID, 'set_active_session.sessionId');
  return value;
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

function requireEqual(actual: string, expected: string, label: string): void {
  if (actual !== expected) {
    throw new Error(
      `${label}: expected ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}.`,
    );
  }
}

function requireBoundRuntime(
  runtimeId: string,
  boundRuntimeId: string,
  command: string,
): void {
  if (!boundRuntimeId || runtimeId !== boundRuntimeId) {
    throw new Error(`${command} used an unbound runtime.`);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export default installPiScenarioAdapter;
