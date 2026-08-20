import { fixtureThinkingLevels, mainSessionState } from './fixtures';
import { savedSessionBootstrap } from './saved-session-stream-then-stale-generation';

import { definePiScenario } from './index';

const runtime = 'main';
const prompt = 'Stop this fixture';
const partialReply = 'Partial reply.';

const savedSessionUnacknowledgedAbort = definePiScenario({
  metadata: {
    name: 'saved-session-unacknowledged-abort',
    purpose:
      'Recover a stopped saved session when Pi never acknowledges its abort.',
    qualityRule:
      'docs/quality.md §5 State correctness: transitional states are bounded and repeat submission is impossible.',
    schemaVersion: 1,
  },
  runtimes: savedSessionBootstrap.runtimes,
  steps: [
    ...savedSessionBootstrap.steps,
    {
      kind: 'request',
      runtime,
      capture: 'prompt',
      match: { type: 'prompt', message: prompt },
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
        assistantMessageEvent: { type: 'text_delta', delta: partialReply },
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'abort',
      match: { type: 'abort' },
    },
    {
      kind: 'gate',
      name: 'abort-request-consumed',
      required: true,
    },
    {
      kind: 'request',
      runtime,
      capture: 'abort-timeout-probe',
      match: { type: 'get_state' },
    },
    {
      kind: 'gate',
      name: 'before-abort-timeout-probe-response',
      required: true,
    },
    {
      kind: 'response',
      request: 'abort-timeout-probe',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'abort-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'abort-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'abort-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'abort-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: prompt },
          {
            role: 'assistant',
            content: [{ type: 'text', text: partialReply }],
          },
        ],
      },
    },
  ],
});

export default savedSessionUnacknowledgedAbort;
