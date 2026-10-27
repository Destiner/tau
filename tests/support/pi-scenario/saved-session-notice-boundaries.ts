import {
  backupSessionState,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario, type PiMessage, type PiScenarioStep } from './index';

const runtime = 'main';
const initialHistory: readonly PiMessage[] = [
  { role: 'user', content: 'Read the invented field notes' },
  {
    role: 'assistant',
    content: [
      {
        type: 'text',
        text: Array.from(
          { length: 24 },
          (_, index) =>
            `Field note ${index + 1}: The synthetic garden is quiet.`,
        ).join('\n\n'),
      },
    ],
  },
];
const firstHistory: readonly PiMessage[] = [
  ...initialHistory,
  { role: 'user', content: 'Inspect the boundary' },
  {
    role: 'assistant',
    content: [{ type: 'text', text: 'Before the notice.After the notice.' }],
  },
];
const finalHistory: readonly PiMessage[] = [
  ...firstHistory,
  { role: 'user', content: 'Continue the inspection' },
  {
    role: 'assistant',
    content: [
      { type: 'text', text: 'A later response arrived in the background.' },
    ],
  },
];

function gate(name: string): PiScenarioStep {
  return { kind: 'gate', name, required: true };
}

function text(delta: string): PiScenarioStep {
  return {
    kind: 'event',
    runtime,
    event: {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta },
    },
  };
}

function notify(id: string, message: string): PiScenarioStep {
  return {
    kind: 'event',
    runtime,
    event: {
      type: 'extension_ui_request',
      id,
      method: 'notify',
      message,
      notifyType: 'warning',
    },
  };
}

function start(capture: string, message: string): readonly PiScenarioStep[] {
  return [
    { kind: 'request', runtime, capture, match: { type: 'prompt', message } },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: `${capture}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${capture}-state`,
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
  ];
}

function hydrate(
  capture: string,
  messages: readonly PiMessage[],
): readonly PiScenarioStep[] {
  return [
    { kind: 'event', runtime, event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime,
      capture: `${capture}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${capture}-state`,
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capture}-messages`,
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime,
      capture: `${capture}-efforts`,
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: `${capture}-efforts`,
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
    gate(`${capture}-pending`),
    {
      kind: 'response',
      request: `${capture}-messages`,
      command: 'get_messages',
      data: { messages },
    },
    gate(`${capture}-complete`),
  ];
}

export default definePiScenario({
  metadata: {
    name: 'saved-session-notice-boundaries',
    purpose:
      'Keep a local notice inside its text gap through ordinary repeated hydration and warm navigation with background output.',
    qualityRule:
      'docs/quality.md §2, §4, §5: draft integrity, physical stability, and session locality.',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'backup', generation: 2 },
  ],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState).map(
      (step) =>
        step.kind === 'response' && step.command === 'get_messages'
          ? { ...step, data: { messages: initialHistory } }
          : step,
    ),
    ...start('first-prompt', 'Inspect the boundary'),
    text('Before the notice.'),
    notify('boundary-warning', 'Keep this warning between the two segments'),
    text('After the notice.'),
    gate('live-gap'),
    { kind: 'response', request: 'first-prompt', command: 'prompt' },
    ...hydrate('first-hydration', firstHistory),
    ...hydrate('repeated-hydration', firstHistory),
    ...start('later-prompt', 'Continue the inspection'),
    gate('later-response-pending'),
    ...successfulBootstrapSteps(
      'backup',
      'backup-bootstrap',
      backupSessionState,
    ).map((step): PiScenarioStep =>
      step.kind === 'response' && step.command === 'get_messages'
        ? {
            ...step,
            data: {
              messages: [
                {
                  role: 'assistant',
                  content: [
                    { type: 'text', text: 'Backup session stays independent.' },
                  ],
                },
              ],
            },
          }
        : step,
    ),
    gate('backup-ready'),
    text('A later response arrived in the background.'),
    notify('background-warning', 'Only Main owns this background warning'),
    { kind: 'response', request: 'later-prompt', command: 'prompt' },
    ...hydrate('later-hydration', finalHistory),
    gate('returned-to-main'),
  ],
});
