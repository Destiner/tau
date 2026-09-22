import {
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const confirmedPrompt = 'Confirm this fixture prompt';
const absentPrompt = 'Reconcile this fixture prompt';
const confirmedReply = 'The first prompt is confirmed.';

const scenario = definePiScenario({
  metadata: {
    name: 'saved-session-prompt-admission',
    purpose:
      'Hold ordinary prompts in admission until direct confirmation or authoritative idle hydration.',
    qualityRule:
      'Repeat submission is impossible while equivalent work is pending',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
    {
      kind: 'request',
      runtime,
      capture: 'confirmed-prompt',
      match: { type: 'prompt', message: confirmedPrompt },
    },
    { kind: 'gate', name: 'optimistic-pending', required: true },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'confirmed-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'confirmed-run-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    { kind: 'response', request: 'confirmed-prompt', command: 'prompt' },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: confirmedReply,
        },
      },
    },
    { kind: 'gate', name: 'prompt-confirmed', required: true },
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: 'confirmed-settled-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'confirmed-settled-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'confirmed-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'confirmed-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'confirmed-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: 'confirmed-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: confirmedPrompt },
          {
            role: 'assistant',
            content: [{ type: 'text', text: confirmedReply }],
          },
        ],
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'absent-prompt',
      match: { type: 'prompt', message: absentPrompt },
    },
    { kind: 'gate', name: 'before-admission-acknowledgement', required: true },
    { kind: 'response', request: 'absent-prompt', command: 'prompt' },
    {
      kind: 'request',
      runtime,
      capture: 'admission-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'admission-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: 'admission-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'admission-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: 'admission-messages',
      match: { type: 'get_messages' },
    },
    { kind: 'gate', name: 'stale-idle-admission', required: true },
    {
      kind: 'response',
      request: 'admission-messages',
      command: 'get_messages',
      data: {
        messages: [
          { role: 'user', content: confirmedPrompt },
          {
            role: 'assistant',
            content: [{ type: 'text', text: confirmedReply }],
          },
        ],
      },
    },
  ],
});

export { absentPrompt, confirmedPrompt };
export default scenario;
