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

const savedSessionBootstrap = definePiScenario({
  metadata: {
    name: 'saved-session-bootstrap',
    purpose: 'Bootstrap a selected saved session into a stable ready UI.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: bootstrapSteps,
});

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
    runtime,
    capture: 'settled-messages',
    match: { type: 'get_messages' },
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
      'Submit through the real composer, stream one reply, and finish settled.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: conversationSteps,
});

const staleGenerationOutput = {
  kind: 'event',
  runtime,
  generation: 1,
  event: {
    type: 'message_update',
    assistantMessageEvent: {
      type: 'text_delta',
      delta: 'STALE_GENERATION_SENTINEL',
    },
  },
} as const;
const staleSteps = [
  ...streamingConversationSteps,
  {
    kind: 'gate',
    name: 'before-stale-generation-output',
    required: true,
  },
  staleGenerationOutput,
] as const;

const savedSessionStaleGeneration = definePiScenario({
  metadata: {
    name: 'saved-session-stale-generation',
    purpose:
      'Pause a working saved session, deliver one stale generation delta, and end without settlement.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: staleSteps,
});

const savedSessionStreamThenStaleGeneration = definePiScenario({
  metadata: {
    name: 'saved-session-stream-then-stale-generation',
    purpose:
      'Bootstrap a saved session, stream one reply, ignore a stale generation delta, and settle.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: [
    ...streamingConversationSteps,
    {
      kind: 'gate',
      name: 'before-stale-generation-output',
      required: true,
    },
    staleGenerationOutput,
    {
      kind: 'gate',
      name: 'after-stale-generation-output',
      required: true,
    },
    ...settledConversationSteps,
  ],
});

export {
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
  savedSessionStreamThenStaleGeneration as default,
};
