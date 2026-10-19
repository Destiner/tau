import {
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const runtimes = [{ key: runtime, generation: 2 }] as const;
const bootstrapSteps = successfulBootstrapSteps(
  runtime,
  'bootstrap',
  mainSessionState,
);

const streamingConversationSteps = [
  ...bootstrapSteps,
  {
    kind: 'request',
    runtime,
    capture: 'prompt',
    match: { type: 'prompt', message: 'Explain the fixture' },
  },
  { kind: 'event', runtime, event: { type: 'agent_start' } },
  {
    kind: 'request',
    runtime,
    capture: 'run-state',
    match: { type: 'get_state' },
  },
  {
    kind: 'response',
    request: 'run-state',
    command: 'get_state',
    data: { ...mainSessionState, isStreaming: true },
  },
  {
    kind: 'event',
    runtime,
    event: {
      type: 'message_update',
      assistantMessageEvent: {
        type: 'text_delta',
        delta: 'Deterministic ',
      },
    },
  },
  {
    kind: 'event',
    runtime,
    event: {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'reply.' },
    },
  },
] as const;

const settledConversationSteps = [
  { kind: 'response', request: 'prompt', command: 'prompt' },
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
    data: { ...mainSessionState, isStreaming: false },
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
        { role: 'user', content: 'Explain the fixture' },
        {
          role: 'assistant',
          content: [{ type: 'text', text: 'Deterministic reply.' }],
        },
      ],
    },
  },
] as const;

const conversationSteps = [
  ...streamingConversationSteps,
  ...settledConversationSteps,
] as const;

const savedSessionConversation = definePiScenario({
  metadata: {
    name: 'saved-session-conversation',
    purpose:
      'Round-trip native Pi stdio requests, stream one reply, and finish settled.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: conversationSteps,
});

export default savedSessionConversation;
