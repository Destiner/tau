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
      'Register a command-created session after its first assistant message_end barrier while the agent keeps working.',
    qualityRule:
      'docs/quality.md §5 State correctness: persisted command sessions become durable without ending an in-progress run.',
    schemaVersion: 1,
    origin:
      'Command-created sessions need transcript evidence before registration',
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
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Real agent work started.',
        },
      },
    },
    {
      kind: 'gate',
      name: 'before-assistant-settlement',
      required: true,
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'Real agent work started.' }],
        },
      },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'message-end-barrier',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'message-end-barrier',
      command: 'get_state',
      data: { ...commandSessionState, isStreaming: true },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'tool_execution_start',
        toolCallId: 'long-tool',
        toolName: 'fixture_wait',
        args: { reason: 'Keep the first run active' },
      },
    },
    {
      kind: 'gate',
      name: 'after-message-end-registration',
      required: true,
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'tool_execution_end',
        toolCallId: 'long-tool',
        toolName: 'fixture_wait',
        result: { content: [{ type: 'text', text: 'Finished waiting.' }] },
        isError: false,
      },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: { type: 'agent_settled' },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'settled-state',
      command: 'get_state',
      data: { ...commandSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'settled-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'settled-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'settled-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'settled-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: 'Run the workflow' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Real agent work started.' }],
          },
        ],
      },
    },
    {
      kind: 'runtime-event',
      runtime: 'phantom',
      event: 'exited',
      code: 0,
    },
  ],
});

export { commandSessionState };
export default phantomCommandRegistration;
