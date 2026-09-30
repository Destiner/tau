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
const planMessages = [
  { role: 'user', content: 'Write the approved plan' },
  {
    role: 'assistant',
    content: [{ type: 'text', text: 'Approved implementation plan.' }],
  },
] as const;

const delayedSuccessorVerification = definePiScenario({
  metadata: {
    name: 'delayed-successor-verification',
    purpose:
      'Keep an empty Implement successor alive after a completed Plan is registered and verification retries are exhausted.',
    qualityRule: 'docs/quality.md §2 Input integrity and §5 State correctness',
    schemaVersion: 1,
  },
  runtimes: [
    { key: runtime, generation: 2 },
    { key: 'backup', generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps(
      runtime,
      'main-bootstrap',
      mainSessionState,
      commands,
    ),
    ...successfulBootstrapSteps(
      'backup',
      'backup-bootstrap',
      backupSessionState,
    ),
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
      capture: 'plan-messages',
      match: { type: 'get_messages' },
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
        message: planMessages[1],
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
      capture: 'plan-settled-messages',
      match: { type: 'get_messages' },
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
      kind: 'response',
      request: 'plan-settled-messages',
      command: 'get_messages',
      data: { messages: planMessages },
    },
    { kind: 'gate', name: 'plan-registered', required: true },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'implement-identity-probe',
      match: { type: 'get_state' },
    },
    { kind: 'gate', name: 'before-implement-identity', required: true },
    {
      kind: 'response',
      request: 'implement-identity-probe',
      command: 'get_state',
      data: { ...implementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-messages',
      match: { type: 'get_messages' },
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
    { kind: 'gate', name: 'before-implement-retry-one', required: true },
    {
      kind: 'response',
      request: 'implement-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-one-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'implement-retry-one-state',
      command: 'get_state',
      data: { ...implementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-one-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-one-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'implement-retry-one-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    { kind: 'gate', name: 'before-implement-retry-two', required: true },
    {
      kind: 'response',
      request: 'implement-retry-one-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-two-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'implement-retry-two-state',
      command: 'get_state',
      data: { ...implementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-two-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-two-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'implement-retry-two-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    { kind: 'gate', name: 'before-implement-retry-three', required: true },
    {
      kind: 'response',
      request: 'implement-retry-two-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-three-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'implement-retry-three-state',
      command: 'get_state',
      data: { ...implementState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-three-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-retry-three-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'implement-retry-three-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'response',
      request: 'implement-retry-three-messages',
      command: 'get_messages',
      data: { messages: [] },
    },
    { kind: 'gate', name: 'before-delayed-successor-start', required: true },
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
    { kind: 'gate', name: 'implement-running', required: true },
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
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'Preparing implementation.' }],
        },
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'implement-materialization-barrier',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'implement-materialization-barrier',
      command: 'get_state',
      data: { ...implementState, isStreaming: true },
    },
    { kind: 'gate', name: 'implement-materialized', required: true },
  ],
});

export default delayedSuccessorVerification;
