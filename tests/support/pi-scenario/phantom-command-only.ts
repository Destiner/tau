import {
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const command = {
  name: 'usage',
  description: 'Show fixture usage',
  source: 'extension',
} as const;
const commandSessionState = {
  ...mainSessionState,
  sessionId: 'session-usage',
  sessionFile: '/fixture/tau-project/session-usage.jsonl',
  sessionName: 'Usage only',
};

const phantomCommandOnly = definePiScenario({
  metadata: {
    name: 'phantom-command-only',
    purpose:
      'Keep a command-only session out of the durable registry and remove it when the user leaves.',
    qualityRule:
      'docs/quality.md §5 State correctness: empty command sessions remain disposable.',
    schemaVersion: 1,
    origin: 'Successful /usage commands could leave durable ghost rows',
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'phantom', generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState, [
      command,
    ]),
    { kind: 'runtime-event', runtime: 'phantom', event: 'started' },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'phantom-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'phantom-commands',
      command: 'get_commands',
      data: { commands: [command] },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'phantom-state',
      command: 'get_state',
      data: { ...commandSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'phantom-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'phantom-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'command-prompt',
      match: { type: 'prompt', message: '/usage' },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'extension_ui_request',
        id: 'usage-notice',
        method: 'notify',
        message: 'Fixture usage is 10%.',
        notifyType: 'info',
      },
    },
    { kind: 'response', request: 'command-prompt', command: 'prompt' },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'command-sync',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'command-sync',
      command: 'get_state',
      data: { ...commandSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'command-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'command-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'command-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'command-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
  ],
});

export default phantomCommandOnly;
