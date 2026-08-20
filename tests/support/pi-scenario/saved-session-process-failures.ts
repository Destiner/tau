import { definePiScenario, type PiScenarioStep } from './index';

const projectPath = '/fixture/tau-project';
const mainSession = {
  sessionId: 'session-main',
  sessionFile: `${projectPath}/session-main.jsonl`,
  sessionName: 'Main',
  model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
  thinkingLevel: 'high',
};
const backupSession = {
  ...mainSession,
  sessionId: 'session-backup',
  sessionFile: `${projectPath}/session-backup.jsonl`,
  sessionName: 'Backup',
};

const rawStderr = 'RAW_STDERR_SECRET_SENTINEL';
const rawBridgeError = 'RAW_BRIDGE_ERROR_SECRET_SENTINEL';
const rawExitMessage = 'Pi exited with status 47. RAW_EXIT_SECRET_SENTINEL';

function successfulBootstrap(
  runtime: string,
  capturePrefix: string,
  state: typeof mainSession,
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
      data: {
        models: [
          {
            provider: 'fixture',
            id: 'alpha',
            name: 'Alpha',
            reasoning: true,
          },
        ],
      },
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
      data: { commands: [] },
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
      data: { levels: ['off', 'high'] },
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

function processFailure(
  runtime: string,
  prefix: string,
): readonly PiScenarioStep[] {
  return [
    { kind: 'gate', name: `before-${prefix}-process-failure`, required: true },
    { kind: 'runtime-event', runtime, event: 'stderr', message: rawStderr },
    {
      kind: 'runtime-event',
      runtime,
      event: 'error',
      message: rawBridgeError,
    },
    { kind: 'gate', name: `after-${prefix}-bridge-error`, required: true },
    {
      kind: 'runtime-event',
      runtime,
      event: 'exited',
      code: 47,
      message: rawExitMessage,
    },
    { kind: 'gate', name: `after-${prefix}-process-exit`, required: true },
  ];
}

const savedSessionBootstrapProcessExit = definePiScenario({
  metadata: {
    name: 'saved-session-bootstrap-process-exit',
    purpose:
      'Bound a raw process failure during bootstrap and reconnect through the selected session.',
    qualityRule: 'State correctness, failure locality, and privacy',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'failed-bootstrap', generation: 2, sessionId: 'session-main' },
    { key: 'recovered-main', generation: 3, sessionId: 'session-main' },
  ],
  steps: [
    { kind: 'runtime-event', runtime: 'failed-bootstrap', event: 'started' },
    ...(['models', 'commands', 'state'] as const).map(
      (capture) =>
        ({
          kind: 'request',
          runtime: 'failed-bootstrap',
          capture: `failed-bootstrap-${capture}`,
          match: {
            type:
              capture === 'models'
                ? 'get_available_models'
                : capture === 'commands'
                  ? 'get_commands'
                  : 'get_state',
          },
        }) as PiScenarioStep,
    ),
    ...processFailure('failed-bootstrap', 'bootstrap'),
    ...successfulBootstrap('recovered-main', 'recovered-main', mainSession),
  ],
});

const savedSessionPromptProcessExit = definePiScenario({
  metadata: {
    name: 'saved-session-prompt-process-exit',
    purpose:
      'Fail one of two sessions after partial output without leaking process details or corrupting its sibling.',
    qualityRule: 'State correctness, failure locality, and privacy',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2, sessionId: 'session-main' },
    { key: 'backup', generation: 1, sessionId: 'session-backup' },
  ],
  steps: [
    ...successfulBootstrap('main', 'main-bootstrap', mainSession),
    ...successfulBootstrap('backup', 'backup-bootstrap', backupSession),
    {
      kind: 'request',
      runtime: 'main',
      capture: 'failing-prompt',
      match: { type: 'prompt', message: 'Fail this fixture' },
    },
    { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'failing-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'failing-run-state',
      command: 'get_state',
      data: { ...mainSession, isStreaming: true },
    },
    {
      kind: 'event',
      runtime: 'main',
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Partial answer before failure.',
        },
      },
    },
    ...processFailure('main', 'prompt'),
  ],
});

export {
  rawBridgeError,
  rawExitMessage,
  rawStderr,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
};
