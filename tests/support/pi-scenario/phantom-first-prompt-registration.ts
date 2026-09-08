import {
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const prompt = 'Keep the first prompt visible';
const reply = 'The first prompt stayed visible.';
const sessionState = {
  ...mainSessionState,
  sessionId: 'session-first-prompt',
  sessionFile: '/fixture/tau-project/session-first-prompt.jsonl',
  sessionName: 'First prompt',
};

const scenario = definePiScenario({
  metadata: {
    name: 'phantom-first-prompt-registration',
    purpose:
      'Keep a phantom session row and its optimistic first prompt continuously visible through identity adoption and registration.',
    qualityRule:
      'docs/quality.md §1 Feedback and §5 State correctness: optimistic UI remains present while asynchronous identity state reconciles.',
    schemaVersion: 1,
    origin: 'Feedback report #3',
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'phantom', generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
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
      data: { commands: [] },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'phantom-state',
      match: { type: 'get_state' },
    },
    { kind: 'gate', name: 'before-first-prompt-identity', required: true },
    {
      kind: 'response',
      request: 'phantom-state',
      command: 'get_state',
      data: { ...sessionState, isStreaming: false },
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
      capture: 'first-prompt',
      match: { type: 'prompt', message: prompt },
    },
    {
      kind: 'gate',
      name: 'after-empty-first-prompt-hydration',
      required: true,
    },
    { kind: 'response', request: 'first-prompt', command: 'prompt' },
    { kind: 'event', runtime: 'phantom', event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: 'run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'run-state',
      command: 'get_state',
      data: { ...sessionState, isStreaming: true },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_start',
        message: { role: 'user', content: prompt },
      },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_end',
        message: { role: 'user', content: prompt },
      },
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
        assistantMessageEvent: { type: 'text_delta', delta: reply },
      },
    },
    {
      kind: 'event',
      runtime: 'phantom',
      event: {
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: reply }],
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
      data: { ...sessionState, isStreaming: true },
    },
    { kind: 'gate', name: 'after-first-prompt-registration', required: true },
    { kind: 'event', runtime: 'phantom', event: { type: 'agent_settled' } },
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
      data: { ...sessionState, isStreaming: false },
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
          { role: 'user', content: prompt },
          {
            role: 'assistant',
            content: [{ type: 'text', text: reply }],
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

export { prompt, reply, sessionState };
export default scenario;
