import {
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'phantom';
const temporaryState = {
  ...mainSessionState,
  sessionId: 'session-command-setup',
  sessionFile: '/fixture/tau-project/session-command-setup.jsonl',
  sessionName: 'Command setup',
};
const replacementState = {
  ...mainSessionState,
  sessionId: 'session-plan-42',
  sessionFile: '/fixture/tau-project/session-plan-42.jsonl',
  sessionName: '42 • plan',
};
const commands = [
  {
    name: 'mock',
    description: 'Open a deterministic workflow phase',
    source: 'extension',
  },
] as const;

const phantomCommandReplacement = definePiScenario({
  metadata: {
    name: 'phantom-command-replacement',
    purpose:
      'Keep a fresh extension-command successor alive after its phantom identity is replaced in the background.',
    qualityRule:
      'docs/quality.md §5 State correctness and the Locality principle',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: runtime, generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps(
      'main',
      'main-bootstrap',
      mainSessionState,
      commands,
    ),
    { kind: 'runtime-event', runtime, event: 'started' },
    {
      kind: 'request',
      runtime,
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
      runtime,
      capture: 'phantom-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'phantom-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: 'phantom-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'phantom-state',
      command: 'get_state',
      data: { ...temporaryState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'phantom-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'phantom-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'phantom-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'response',
      request: 'phantom-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    {
      kind: 'request',
      runtime,
      capture: 'command-prompt',
      match: { type: 'prompt', message: '/mock 42' },
    },
    {
      kind: 'gate',
      name: 'before-command-prompt-reply',
      required: true,
    },
    { kind: 'response', request: 'command-prompt', command: 'prompt' },
    {
      kind: 'request',
      runtime,
      capture: 'command-identity-probe',
      match: { type: 'get_state' },
    },
    {
      kind: 'gate',
      name: 'before-command-sync-state-reply',
      required: true,
    },
    {
      kind: 'response',
      request: 'command-identity-probe',
      command: 'get_state',
      data: { ...replacementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'replacement-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'replacement-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'replacement-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'replacement-run-state',
      command: 'get_state',
      data: { ...replacementState, isStreaming: true },
    },
    { kind: 'event', runtime, event: { type: 'turn_start' } },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_start',
        message: { role: 'user', content: 'Run phase 42' },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_end',
        message: { role: 'user', content: 'Run phase 42' },
      },
    },
    { kind: 'gate', name: 'before-live-user-hydration', required: true },
    {
      kind: 'response',
      request: 'replacement-messages',
      command: 'get_messages',
      data: { messages: [{ role: 'user', content: 'Run phase 42' }] },
    },
    { kind: 'gate', name: 'before-replacement-assistant', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'tool_execution_start',
        toolCallId: 'phase-tool',
        toolName: 'workflow',
        args: { phase: 42 },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'tool_execution_end',
        toolCallId: 'phase-tool',
        toolName: 'workflow',
        result: { content: [{ type: 'text', text: 'Phase ready' }] },
        isError: false,
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Replacement session started.',
        },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'Replacement session started.' }],
        },
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'replacement-materialization-barrier',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'replacement-materialization-barrier',
      command: 'get_state',
      data: { ...replacementState, isStreaming: true },
    },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'settled-state',
      command: 'get_state',
      data: { ...replacementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'settled-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
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
      kind: 'response',
      request: 'settled-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: 'Run phase 42' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Replacement session started.' }],
          },
        ],
      },
    },
    { kind: 'gate', name: 'after-settled-hydration', required: true },
  ],
});

export default phantomCommandReplacement;
