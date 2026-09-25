import {
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const initialPrompt = 'Hold the tool turn';
const steeringMessages = [
  'Steering message one',
  'Steering message two',
  'Steering message three',
] as const;
const followUpMessages = [
  'Follow-up message one',
  'Follow-up message two',
] as const;

const savedSessionSteeringBoundary = definePiScenario({
  metadata: {
    name: 'saved-session-steering-boundary',
    purpose:
      'Deliver queued steering together at a tool boundary and follow-ups one turn at a time.',
    qualityRule:
      'docs/quality.md §5 State correctness: each queued message appears exactly once and in order.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'main-bootstrap', mainSessionState),
    {
      kind: 'request',
      runtime,
      capture: 'initial-prompt',
      match: { type: 'prompt', message: initialPrompt },
    },
    { kind: 'response', request: 'initial-prompt', command: 'prompt' },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'held-turn-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'held-turn-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'tool_execution_start',
        toolCallId: 'held-tool',
        toolName: 'wait',
        args: { until: 'steering boundary' },
      },
    },
    { kind: 'gate', name: 'tool-turn-held', required: true },
    {
      kind: 'request',
      runtime,
      capture: 'steering-mode',
      match: { type: 'set_steering_mode', mode: 'all' },
    },
    {
      kind: 'response',
      request: 'steering-mode',
      command: 'set_steering_mode',
    },
    {
      kind: 'request',
      runtime,
      capture: 'follow-up-mode',
      match: { type: 'set_follow_up_mode', mode: 'one-at-a-time' },
    },
    {
      kind: 'response',
      request: 'follow-up-mode',
      command: 'set_follow_up_mode',
    },
    ...steeringMessages.flatMap((message, index) => [
      {
        kind: 'request' as const,
        runtime,
        capture: `steering-${index}`,
        match: {
          type: 'prompt' as const,
          message,
          streamingBehavior: 'steer' as const,
        },
      },
      {
        kind: 'response' as const,
        request: `steering-${index}`,
        command: 'prompt' as const,
      },
      {
        kind: 'event' as const,
        runtime,
        event: {
          type: 'queue_update' as const,
          steering: steeringMessages.slice(0, index + 1),
          followUp: [],
        },
      },
    ]),
    { kind: 'gate', name: 'steering-queued', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'tool_execution_end',
        toolCallId: 'held-tool',
        toolName: 'wait',
        result: { content: [{ type: 'text', text: 'Boundary reached' }] },
        isError: false,
      },
    },
    {
      kind: 'event',
      runtime,
      event: { type: 'queue_update', steering: [], followUp: [] },
    },
    ...steeringMessages.flatMap((content) => [
      {
        kind: 'event' as const,
        runtime,
        event: {
          type: 'message_start' as const,
          message: { role: 'user' as const, content },
        },
      },
      {
        kind: 'event' as const,
        runtime,
        event: {
          type: 'message_end' as const,
          message: { role: 'user' as const, content },
        },
      },
    ]),
    { kind: 'gate', name: 'steering-boundary-delivered', required: true },
    ...followUpMessages.flatMap((message, index) => [
      {
        kind: 'request' as const,
        runtime,
        capture: `follow-up-${index}`,
        match: {
          type: 'prompt' as const,
          message,
          streamingBehavior: 'followUp' as const,
        },
      },
      {
        kind: 'response' as const,
        request: `follow-up-${index}`,
        command: 'prompt' as const,
      },
      {
        kind: 'event' as const,
        runtime,
        event: {
          type: 'queue_update' as const,
          steering: [],
          followUp: followUpMessages.slice(0, index + 1),
        },
      },
    ]),
    { kind: 'gate', name: 'follow-ups-queued', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'queue_update',
        steering: [],
        followUp: [followUpMessages[1]],
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_start',
        message: { role: 'user', content: followUpMessages[0] },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_end',
        message: { role: 'user', content: followUpMessages[0] },
      },
    },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'first-follow-up-settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'first-follow-up-settled-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'first-follow-up-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'first-follow-up-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'first-follow-up-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'first-follow-up-messages',
      command: 'get_messages',
      data: {
        messages: [
          ...steeringMessages.map((content) => ({
            role: 'user' as const,
            content,
          })),
          { role: 'user', content: followUpMessages[0] },
        ],
      },
    },
    { kind: 'gate', name: 'first-follow-up-hydrated', required: true },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'second-follow-up-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'second-follow-up-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    {
      kind: 'event',
      runtime,
      event: { type: 'queue_update', steering: [], followUp: [] },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_start',
        message: { role: 'user', content: followUpMessages[1] },
      },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_end',
        message: { role: 'user', content: followUpMessages[1] },
      },
    },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'second-follow-up-settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'second-follow-up-settled-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'second-follow-up-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'second-follow-up-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'second-follow-up-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'second-follow-up-messages',
      command: 'get_messages',
      data: {
        messages: [
          ...steeringMessages.map((content) => ({
            role: 'user' as const,
            content,
          })),
          ...followUpMessages.map((content) => ({
            role: 'user' as const,
            content,
          })),
        ],
      },
    },
    { kind: 'gate', name: 'second-follow-up-hydrated', required: true },
    { kind: 'runtime-event', runtime, event: 'exited', code: 0 },
  ],
});

export { followUpMessages, initialPrompt, steeringMessages };
export default savedSessionSteeringBoundary;
