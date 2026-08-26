import {
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const runtimes = [{ key: runtime, generation: 2 }] as const;
const firstPrompt = 'Compact this fixture';
const secondPrompt = 'Miss the compaction start';
const summary = 'Earlier fixture context.';

const settlement = (
  prefix: string,
  messages: readonly (
    | { role: 'user'; content: string }
    | { role: 'compactionSummary'; summary: string }
    | {
        role: 'assistant';
        content: readonly { type: 'text'; text: string }[];
      }
  )[],
) =>
  [
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: `${prefix}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${prefix}-state`,
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false, isCompacting: false },
    },
    {
      kind: 'request',
      runtime,
      capture: `${prefix}-efforts`,
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: `${prefix}-efforts`,
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    {
      kind: 'request',
      runtime,
      capture: `${prefix}-messages`,
      match: { type: 'get_messages' },
    },
    {
      kind: 'response',
      request: `${prefix}-messages`,
      command: 'get_messages',
      data: { messages },
    },
  ] as const;

const scenario = definePiScenario({
  metadata: {
    name: 'saved-session-compaction',
    purpose:
      'Show active compaction from both its event and reconciled Pi state, then hydrate one permanent boundary.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes,
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
    {
      kind: 'request',
      runtime,
      capture: 'first-prompt',
      match: { type: 'prompt', message: firstPrompt },
    },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'first-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'first-run-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true, isCompacting: false },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Ready to compact.',
        },
      },
    },
    { kind: 'event', runtime, event: { type: 'compaction_start' } },
    { kind: 'gate', name: 'compaction-started', required: true },
    {
      kind: 'event',
      runtime,
      event: { type: 'compaction_end', result: { summary } },
    },
    { kind: 'response', request: 'first-prompt', command: 'prompt' },
    ...settlement('first-settled', [
      { role: 'compactionSummary', summary },
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'Ready to compact.' }],
      },
    ]),
    {
      kind: 'request',
      runtime,
      capture: 'second-prompt',
      match: { type: 'prompt', message: secondPrompt },
    },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'second-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'second-run-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true, isCompacting: true },
    },
    { kind: 'gate', name: 'missed-start-reconciled', required: true },
    {
      kind: 'event',
      runtime,
      event: { type: 'compaction_end', result: { summary } },
    },
    { kind: 'response', request: 'second-prompt', command: 'prompt' },
    ...settlement('second-settled', [
      { role: 'compactionSummary', summary },
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'Ready to compact.' }],
      },
      { role: 'user', content: secondPrompt },
    ]),
  ],
});

export default scenario;
