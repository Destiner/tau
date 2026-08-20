import { definePiScenario } from './index';

const runtime = 'main';
const state = {
  sessionId: 'session-main',
  sessionFile: '/fixture/tau-project/session-main.jsonl',
  sessionName: 'Main',
  model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
  thinkingLevel: 'high',
};

const savedSessionStreamThenStaleGeneration = definePiScenario({
  metadata: {
    name: 'saved-session-stream-then-stale-generation',
    purpose:
      'Bootstrap a saved session, stream one reply, settle, and ignore a stale generation delta.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2, sessionId: 'session-main' }],
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
  ],
});

export default savedSessionStreamThenStaleGeneration;
