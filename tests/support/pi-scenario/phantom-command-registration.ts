import {
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const command = {
  name: 'mcp',
  description: 'Open the fixture command session',
  source: 'extension',
} as const;
const commandSessionState = {
  ...mainSessionState,
  sessionId: 'session-mcp',
  sessionFile: '/fixture/tau-project/session-mcp.jsonl',
  sessionName: 'MCP workflow',
};

const phantomCommandRegistration = definePiScenario({
  metadata: {
    name: 'phantom-command-registration',
    purpose:
      'Register a command-created session even when its immediate identity sync remains streaming.',
    qualityRule:
      'docs/quality.md §5 State correctness: unsaved session identity and lifecycle transitions stay recoverable.',
    schemaVersion: 1,
    origin: 'Historical /mcp phantom session registration failure',
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
      match: { type: 'prompt', message: '/mcp' },
    },
    { kind: 'response', request: 'command-prompt', command: 'prompt' },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'streaming-command-sync',
      match: { type: 'get_state' },
    },
    {
      kind: 'gate',
      name: 'before-streaming-command-sync',
      required: true,
    },
    {
      kind: 'response',
      request: 'streaming-command-sync',
      command: 'get_state',
      data: { ...commandSessionState, isStreaming: true },
    },
  ],
});

export { commandSessionState };
export default phantomCommandRegistration;
