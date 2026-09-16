import {
  backupSessionState,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario, type PiScenarioStep } from './index';

const rawStderr = 'RAW_STDERR_SECRET_SENTINEL';
const rawBridgeError = 'RAW_BRIDGE_ERROR_SECRET_SENTINEL';
const rawExitMessage = 'Pi exited with status 47. RAW_EXIT_SECRET_SENTINEL';

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
    { key: 'failed-bootstrap', generation: 2 },
    { key: 'recovered-main', generation: 3 },
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
    ...successfulBootstrapSteps(
      'recovered-main',
      'recovered-main',
      mainSessionState,
    ),
  ],
});

const remotePhantomPromptProcessExit = definePiScenario({
  metadata: {
    name: 'remote-phantom-prompt-process-exit',
    purpose:
      'Restore a remote phantom first prompt when startup errors then exits, while keeping retry available.',
    qualityRule: 'State correctness, failure locality, and input integrity',
    schemaVersion: 1,
    origin: 'Feedback report #5',
  },
  runtimes: [
    { key: 'main', generation: 1 },
    { key: 'failed-phantom', generation: 2 },
    { key: 'recovered-phantom', generation: 3 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
    { kind: 'runtime-event', runtime: 'failed-phantom', event: 'started' },
    ...(['models', 'commands', 'state'] as const).map(
      (capture) =>
        ({
          kind: 'request',
          runtime: 'failed-phantom',
          capture: `failed-phantom-${capture}`,
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
    ...processFailure('failed-phantom', 'remote-phantom-prompt'),
    ...successfulBootstrapSteps('recovered-phantom', 'recovered-phantom', {
      ...mainSessionState,
      sessionId: 'session-recovered-phantom',
      sessionFile: '/remote/tau-project/session-recovered-phantom.jsonl',
      sessionName: 'Recovered phantom',
    }),
  ],
});

const remoteSavedSessionProcessExit = definePiScenario({
  metadata: {
    name: 'remote-saved-session-process-exit',
    purpose:
      'Reconnect an established remote saved session after its SSH bridge exits.',
    qualityRule: 'State correctness, failure locality, and input integrity',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 1 },
    { key: 'reconnected-main', generation: 2 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
    ...processFailure('main', 'remote-saved-session'),
    ...successfulBootstrapSteps(
      'reconnected-main',
      'reconnected-main',
      mainSessionState,
    ),
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
    { key: 'main', generation: 2 },
    { key: 'backup', generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
    ...successfulBootstrapSteps(
      'backup',
      'backup-bootstrap',
      backupSessionState,
    ),
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
      data: { ...mainSessionState, isStreaming: true },
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
  remotePhantomPromptProcessExit,
  remoteSavedSessionProcessExit,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
};
