import {
  backupSessionState,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario, type PiScenarioStep } from './index';

const runtime = 'main';
const initialPrompt = 'Start work before compacting';
const steeringMessage = 'Steer during automatic compaction';
const clickedMessage = 'Queue with the automatic compaction button';
const followUpMessage = 'Follow up after automatic compaction';
const exactDraft =
  '  Preserve this draft exactly\nincluding its final newline\n';

function runSteps(): readonly PiScenarioStep[] {
  return [
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
      capture: 'run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'run-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true, isCompacting: false },
    },
  ];
}

function modeSteps(): readonly PiScenarioStep[] {
  return [
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
  ];
}

function queueSteps(
  capture: string,
  message: string,
  streamingBehavior: 'steer' | 'followUp',
  steering: readonly string[],
  followUp: readonly string[],
): readonly PiScenarioStep[] {
  return [
    {
      kind: 'request',
      runtime,
      capture,
      match: { type: 'prompt', message, streamingBehavior },
    },
    { kind: 'response', request: capture, command: 'prompt' },
    {
      kind: 'event',
      runtime,
      event: { type: 'queue_update', steering, followUp },
    },
    { kind: 'gate', name: `${capture}-visible`, required: true },
  ];
}

const automaticCompactionQueueScenarios = (
  ['threshold', 'overflow'] as const
).map((reason) =>
  definePiScenario({
    metadata: {
      name: `saved-session-compaction-queue-${reason}`,
      purpose: `Queue steering and follow-ups during active ${reason} compaction without stopping Pi.`,
      qualityRule:
        'State correctness: active automatic compaction preserves queue delivery.',
      schemaVersion: 1,
    },
    runtimes: [{ key: runtime, generation: 2 }],
    steps: [
      ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
      ...runSteps(),
      { kind: 'event', runtime, event: { type: 'compaction_start', reason } },
      { kind: 'gate', name: 'compaction-ready', required: true },
      ...modeSteps(),
      ...queueSteps('steer', steeringMessage, 'steer', [steeringMessage], []),
      ...queueSteps(
        'clicked',
        clickedMessage,
        'steer',
        [steeringMessage, clickedMessage],
        [],
      ),
      ...queueSteps(
        'follow-up',
        followUpMessage,
        'followUp',
        [steeringMessage, clickedMessage],
        [followUpMessage],
      ),
    ],
  }),
);

const blockedCompactionCases = [
  { key: 'manual', reason: 'manual', active: true },
  { key: 'missing', reason: undefined, active: true },
  { key: 'unknown', reason: 'future-reason', active: true },
  { key: 'invalid', reason: 42, active: true },
  { key: 'pre-run-threshold', reason: 'threshold', active: false },
  { key: 'pre-run-overflow', reason: 'overflow', active: false },
] as const;

const blockedCompactionQueueScenarios = blockedCompactionCases.map(
  ({ key, reason, active }) =>
    definePiScenario({
      metadata: {
        name: `saved-session-compaction-queue-blocked-${key}`,
        purpose: `Keep an editable draft without sending during ${key} compaction.`,
        qualityRule:
          'State correctness: unsupported compaction never admits a draft.',
        schemaVersion: 1,
      },
      runtimes: [{ key: runtime, generation: 2 }],
      steps: [
        ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
        ...(active ? runSteps() : []),
        {
          kind: 'event',
          runtime,
          event: {
            type: 'compaction_start',
            ...(reason === undefined ? {} : { reason }),
          },
        },
        { kind: 'gate', name: 'compaction-ready', required: true },
        { kind: 'event', runtime, event: { type: 'compaction_end' } },
        { kind: 'gate', name: 'compaction-finished', required: true },
      ],
    }),
);

const compactionQueueRaceCases = [
  'manual',
  'reconciliation',
  'settlement',
] as const;

const compactionQueueRaceScenarios = compactionQueueRaceCases.map((race) =>
  definePiScenario({
    metadata: {
      name: `saved-session-compaction-queue-race-${race}`,
      purpose: `Cancel a provisional queue draft when ${race} overtakes delayed queue mode setup, without replay.`,
      qualityRule:
        'State correctness: asynchronous admission is rechecked and exact drafts survive cancellation.',
      schemaVersion: 1,
    },
    runtimes: [{ key: runtime, generation: 2 }],
    steps: [
      ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
      ...runSteps(),
      {
        kind: 'event',
        runtime,
        event: { type: 'compaction_start', reason: 'threshold' },
      },
      { kind: 'gate', name: 'compaction-ready', required: true },
      ...modeSteps().slice(0, 3),
      { kind: 'gate', name: 'queue-mode-pending', required: true },
      ...(race === 'manual'
        ? [
            {
              kind: 'event',
              runtime,
              event: { type: 'compaction_start', reason: 'manual' },
            } as const,
          ]
        : race === 'reconciliation'
          ? [
              {
                kind: 'event',
                runtime,
                event: {
                  type: 'compaction_end',
                  result: { summary: 'Earlier fixture context.' },
                },
              } as const,
              {
                kind: 'request',
                runtime,
                capture: 'reconciled-messages',
                match: { type: 'get_messages' },
              } as const,
            ]
          : [
              {
                kind: 'event',
                runtime,
                event: { type: 'agent_settled' },
              } as const,
              {
                kind: 'request',
                runtime,
                capture: 'settled-state',
                match: { type: 'get_state' },
              } as const,
              {
                kind: 'response',
                request: 'settled-state',
                command: 'get_state',
                data: {
                  ...mainSessionState,
                  isStreaming: false,
                  isCompacting: false,
                },
              } as const,
              {
                kind: 'request',
                runtime,
                capture: 'settled-messages',
                match: { type: 'get_messages' },
              } as const,
              {
                kind: 'request',
                runtime,
                capture: 'settled-efforts',
                match: { type: 'get_available_thinking_levels' },
              } as const,
              {
                kind: 'response',
                request: 'settled-efforts',
                command: 'get_available_thinking_levels',
                data: { levels: fixtureThinkingLevels },
              } as const,
              {
                kind: 'response',
                request: 'settled-messages',
                command: 'get_messages',
                data: { messages: [{ role: 'user', content: initialPrompt }] },
              } as const,
            ]),
      ...modeSteps().slice(3),
      { kind: 'gate', name: 'queue-mode-resolved', required: true },
      ...(race === 'reconciliation'
        ? [
            {
              kind: 'response',
              request: 'reconciled-messages',
              command: 'get_messages',
              data: { messages: [{ role: 'user', content: initialPrompt }] },
            } as const,
          ]
        : race === 'manual'
          ? [
              {
                kind: 'event',
                runtime,
                event: { type: 'compaction_end' },
              } as const,
            ]
          : []),
      { kind: 'gate', name: 'admission-restored', required: true },
    ],
  }),
);

const compactionQueueNavigationScenarios = (
  ['threshold', 'manual'] as const
).map((reason) =>
  definePiScenario({
    metadata: {
      name: `saved-session-compaction-queue-navigation-${reason}`,
      purpose:
        'Keep compaction admission, queues, and exact drafts local across session switching.',
      qualityRule:
        'State correctness: session navigation never leaks compaction or loses drafts.',
      schemaVersion: 1,
    },
    runtimes: [
      { key: runtime, generation: 2 },
      { key: 'backup', generation: 2 },
    ],
    steps: [
      ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
      { kind: 'event', runtime, event: { type: 'compaction_start', reason } },
      { kind: 'gate', name: 'compaction-ready', required: true },

      ...successfulBootstrapSteps(
        'backup',
        'backup-bootstrap',
        backupSessionState,
      ),
      { kind: 'gate', name: 'backup-ready', required: true },
    ],
  }),
);

function compactionQueueNativeProfile(name: string): string {
  if (name.includes('-navigation-'))
    return 'saved-session-extension-prompt-cycling';
  if (name.includes('pre-run-')) return 'saved-session-bootstrap';
  if (name.endsWith('settlement')) return 'saved-session-conversation';
  return 'saved-session-message-queue';
}

const compactionQueueScenarios = [
  ...compactionQueueNavigationScenarios,
  ...automaticCompactionQueueScenarios,
  ...blockedCompactionQueueScenarios,
  ...compactionQueueRaceScenarios,
];

export {
  blockedCompactionCases,
  clickedMessage,
  compactionQueueRaceCases,
  compactionQueueNativeProfile,
  exactDraft,
  followUpMessage,
  initialPrompt,
  steeringMessage,
};
export default compactionQueueScenarios;
