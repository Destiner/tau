import { definePiScenario } from './index';

const runtime = 'main';
const state = {
  sessionId: 'session-main',
  sessionFile: '/fixture/tau-project/session-main.jsonl',
  sessionName: 'Main',
  model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
  thinkingLevel: 'high',
};

const runtimes = [
  { key: runtime, generation: 2, sessionId: 'session-main' },
] as const;
const bootstrapSteps = [
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
    capture: 'bootstrap-commands',
    match: { type: 'get_commands' },
  },
  {
    kind: 'response',
    request: 'bootstrap-commands',
    command: 'get_commands',
    data: { commands: [] },
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
    data: { ...state, isStreaming: false },
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
    data: { levels: ['off', 'high'] },
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
] as const;

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
    data: { ...state, isStreaming: true },
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
    data: { ...state, isStreaming: false },
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
    data: { levels: ['off', 'high'] },
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
    {
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
    },
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
  savedSessionStreamThenStaleGeneration as default,
};
