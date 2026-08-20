import type { PiScenarioStep, PiState } from './index';

const fixtureProjectPath = '/fixture/tau-project';
const fixtureModels = [
  {
    provider: 'fixture',
    id: 'alpha',
    name: 'Alpha',
    reasoning: true,
  },
] as const;
const fixtureThinkingLevels = ['off', 'high'] as const;
const mainSessionState = {
  sessionId: 'session-main',
  sessionFile: `${fixtureProjectPath}/session-main.jsonl`,
  sessionName: 'Main',
  model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
  thinkingLevel: 'high',
} as const;
const backupSessionState = {
  ...mainSessionState,
  sessionId: 'session-backup',
  sessionFile: `${fixtureProjectPath}/session-backup.jsonl`,
  sessionName: 'Backup',
} as const;

function successfulBootstrapSteps(
  runtime: string,
  capturePrefix: string,
  state: Omit<PiState, 'isStreaming'>,
  commands: readonly {
    name: string;
    description?: string;
    source?: string;
  }[] = [],
): readonly PiScenarioStep[] {
  return [
    { kind: 'runtime-event', runtime, event: 'started' },
    {
      kind: 'request',
      runtime,
      capture: `${capturePrefix}-models`,
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: `${capturePrefix}-models`,
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capturePrefix}-commands`,
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: `${capturePrefix}-commands`,
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capturePrefix}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${capturePrefix}-state`,
      command: 'get_state',
      data: { ...state, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capturePrefix}-efforts`,
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: `${capturePrefix}-efforts`,
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capturePrefix}-messages`,
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: `${capturePrefix}-messages`,
      command: 'get_messages',
      data: { messages: [] },
    },
  ];
}

export {
  backupSessionState,
  fixtureModels,
  fixtureProjectPath,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
};
