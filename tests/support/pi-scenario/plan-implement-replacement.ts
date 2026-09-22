import {
  backupSessionState,
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'workflow';
const commands = [
  {
    name: 'mock-workflow',
    description: 'Open deterministic Plan and Implement phases',
    source: 'extension',
  },
] as const;
const planState = {
  ...mainSessionState,
  sessionId: 'session-plan',
  sessionFile: '/fixture/tau-project/session-plan.jsonl',
  sessionName: 'docs · RHI-6267 · Plan',
};
const implementState = {
  ...mainSessionState,
  sessionId: 'session-implement',
  sessionFile: '/fixture/tau-project/session-implement.jsonl',
  sessionName: 'docs · RHI-6267 · Implement',
};

const planImplementReplacement = definePiScenario({
  metadata: {
    name: 'plan-implement-replacement',
    purpose:
      'Preserve a completed Plan when a delayed settlement probe discovers Implement.',
    qualityRule:
      'docs/quality.md §5 State correctness and the Never lose the user’s work principle',
    schemaVersion: 1,
  },
  runtimes: [
    { key: runtime, generation: 2 },
    { key: 'backup', generation: 1 },
    { key: 'reopened-plan', generation: 1 },
  ],
  steps: [
    { kind: 'runtime-event', runtime, event: 'started' },
    {
      kind: 'request',
      runtime,
      capture: 'bootstrap-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'bootstrap-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'bootstrap-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'bootstrap-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: 'bootstrap-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'bootstrap-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'bootstrap-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'bootstrap-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'bootstrap-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'bootstrap-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    ...successfulBootstrapSteps('backup', 'backup', backupSessionState),
    {
      kind: 'request',
      runtime,
      capture: 'workflow-command',
      match: { type: 'prompt', message: '/mock-workflow' },
    },
    { kind: 'response', request: 'workflow-command', command: 'prompt' },
    {
      kind: 'request',
      runtime,
      capture: 'plan-identity',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'plan-identity',
      command: 'get_state',
      data: { ...planState, isStreaming: true },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'plan-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'plan-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'plan-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-messages',
      match: { type: 'get_messages' },
    },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'plan-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'plan-run-state',
      command: 'get_state',
      data: { ...planState, isStreaming: true },
    },
    { kind: 'event', runtime, event: { type: 'turn_start' } },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_start',
        message: { role: 'user', content: 'Write the approved plan' },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_end',
        message: { role: 'user', content: 'Write the approved plan' },
      },
    },
    {
      kind: 'response',
      request: 'plan-messages',
      command: 'get_messages',
      data: {
        messages: [{ role: 'user', content: 'Write the approved plan' }],
      },
    },
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
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Approved implementation plan.',
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
          content: [{ type: 'text', text: 'Approved implementation plan.' }],
        },
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-materialization-barrier',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'plan-materialization-barrier',
      command: 'get_state',
      data: { ...planState, isStreaming: true },
    },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'plan-settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'plan-settled-state',
      command: 'get_state',
      data: { ...planState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-settled-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'plan-settled-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'plan-settled-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-probe',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'plan-settled-messages',
      command: 'get_messages',
      // Production streamed the final assistant event before persistence made
      // it visible to the overlapping settlement hydration.
      data: {
        messages: [{ role: 'user', content: 'Write the approved plan' }],
      },
    },
    { kind: 'gate', name: 'plan-hydration-lagged', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'session_info_changed',
        name: implementState.sessionName,
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-name-refresh',
      match: { type: 'get_state' },
    },
    { kind: 'gate', name: 'before-implement-identity', required: true },
    {
      kind: 'response',
      request: 'implement-probe',
      command: 'get_state',
      data: { ...implementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'implement-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'implement-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'implement-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'implement-messages',
      command: 'get_messages',
      data: {
        messages: [{ role: 'user', content: 'Implement the approved plan' }],
      },
    },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'implement-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'implement-run-state',
      command: 'get_state',
      data: { ...implementState, isStreaming: true },
    },
    { kind: 'gate', name: 'implement-active', required: true },
    { kind: 'runtime-event', runtime: 'reopened-plan', event: 'started' },
    {
      kind: 'request',
      runtime: 'reopened-plan',
      capture: 'reopened-plan-models',
      match: { type: 'get_available_models' },
    },
    {
      kind: 'response',
      request: 'reopened-plan-models',
      command: 'get_available_models',
      data: { models: fixtureModels },
    },
    {
      kind: 'request',
      runtime: 'reopened-plan',
      capture: 'reopened-plan-commands',
      match: { type: 'get_commands' },
    },
    {
      kind: 'response',
      request: 'reopened-plan-commands',
      command: 'get_commands',
      data: { commands },
    },
    {
      kind: 'request',
      runtime: 'reopened-plan',
      capture: 'reopened-plan-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'reopened-plan-state',
      command: 'get_state',
      data: { ...planState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime: 'reopened-plan',
      capture: 'reopened-plan-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'reopened-plan-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime: 'reopened-plan',
      capture: 'reopened-plan-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'reopened-plan-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: 'Write the approved plan' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Approved implementation plan.' }],
          },
        ],
      },
    },
  ],
});

export default planImplementReplacement;
