import {
  backupSessionState,
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario, type PiMessage, type PiScenarioStep } from './index';

const helper = {
  name: 'tau-internal-reload-v1',
  description: 'Private Tau reload helper',
  source: 'extension',
};
const beforeCommands = [
  helper,
  { name: 'old-resource', description: 'Old resource', source: 'extension' },
];
const afterCommands = [
  helper,
  { name: 'new-resource', description: 'New resource', source: 'extension' },
];
const history: PiMessage[] = [
  { role: 'user', content: 'Keep this conversation' },
  {
    role: 'assistant',
    content: [
      { type: 'text', text: 'The existing conversation stays intact.' },
    ],
  },
];

function bootstrap(runtime = 'main'): readonly PiScenarioStep[] {
  return successfulBootstrapSteps(
    runtime,
    `${runtime}-bootstrap`,
    runtime === 'backup' ? backupSessionState : mainSessionState,
    beforeCommands,
  ).map((step) =>
    step.kind === 'response' && step.command === 'get_messages'
      ? { ...step, data: { messages: history } }
      : step,
  );
}

function reloadSteps(prefix: string): readonly PiScenarioStep[] {
  return [
    {
      kind: 'request',
      runtime: 'main',
      capture: `${prefix}-prompt`,
      match: { type: 'prompt', message: '/tau-internal-reload-v1' },
    },
    { kind: 'gate', name: `${prefix}-pending`, required: true },
    {
      kind: 'response',
      request: `${prefix}-prompt`,
      command: 'prompt',
      data: { disposition: 'handled' },
    },
    {
      kind: 'request',
      runtime: 'main',
      capture: `${prefix}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${prefix}-state`,
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: false },
    },
    ...(['get_messages', 'get_available_thinking_levels'] as const).flatMap(
      (type): PiScenarioStep[] => [
        {
          kind: 'request',
          runtime: 'main',
          capture: `${prefix}-ordinary-${type}`,
          match: { type },
        },
        ...(type === 'get_messages'
          ? [
              {
                kind: 'response' as const,
                request: `${prefix}-ordinary-${type}`,
                command: type,
                data: { messages: history },
              },
            ]
          : [
              {
                kind: 'response' as const,
                request: `${prefix}-ordinary-${type}`,
                command: type,
                data: { levels: fixtureThinkingLevels },
              },
            ]),
      ],
    ),
    ...(
      [
        'get_commands',
        'get_available_models',
        'get_available_thinking_levels',
        'get_messages',
      ] as const
    ).map((type): PiScenarioStep => ({
      kind: 'request',
      runtime: 'main',
      capture: `${prefix}-${type}`,
      match: { type },
    })),
    {
      kind: 'response',
      request: `${prefix}-get_commands`,
      command: 'get_commands',
      data: { commands: afterCommands },
    },
    {
      kind: 'response',
      request: `${prefix}-get_available_models`,
      command: 'get_available_models',
      data: {
        models: [
          ...fixtureModels,
          {
            provider: 'fixture',
            id: 'beta',
            name: 'Reloaded Beta',
            reasoning: true,
          },
        ],
      },
    },
    {
      kind: 'response',
      request: `${prefix}-get_messages`,
      command: 'get_messages',
      data: { messages: history },
    },
    { kind: 'gate', name: `${prefix}-refreshing`, required: true },
    {
      kind: 'response',
      request: `${prefix}-get_available_thinking_levels`,
      command: 'get_available_thinking_levels',
      data: { levels: [...fixtureThinkingLevels, 'medium'] },
    },
  ];
}

const reloadResources = definePiScenario({
  metadata: {
    name: 'reload-resources',
    purpose:
      'Reload twice without changing identity or history, holding progress until every resource refresh completes.',
    qualityRule:
      'docs/quality.md §2 Input integrity, §5 State correctness, §6 Keyboard and focus',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'backup', generation: 1 },
  ],
  steps: [
    ...bootstrap(),
    ...reloadSteps('first'),
    ...bootstrap('backup'),
    ...reloadSteps('second'),
  ],
});

const reloadRejected = definePiScenario({
  metadata: {
    name: 'reload-rejected',
    purpose:
      'Reject a reload without sending a model prompt or destroying the editable draft.',
    qualityRule: 'docs/quality.md §2 Input integrity, §3 Failure',
    schemaVersion: 1,
  },
  runtimes: [{ key: 'main', generation: 2 }],
  steps: [
    ...bootstrap(),
    {
      kind: 'request',
      runtime: 'main',
      capture: 'reload',
      match: { type: 'prompt', message: '/tau-internal-reload-v1' },
    },
    { kind: 'gate', name: 'reload-pending', required: true },
    {
      kind: 'response',
      request: 'reload',
      command: 'prompt',
      success: false,
      error: 'PRIVATE_RELOAD_FAILURE_CANARY',
    },
  ],
});

const reloadBusy = definePiScenario({
  metadata: {
    name: 'reload-busy',
    purpose:
      'Refuse reload during response generation and compaction without queueing or replaying it.',
    qualityRule: 'docs/quality.md §2 Input integrity, §5 State correctness',
    schemaVersion: 1,
  },
  runtimes: [{ key: 'main', generation: 2 }],
  steps: [
    ...bootstrap(),
    { kind: 'gate', name: 'before-streaming', required: true },
    { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'stream-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'stream-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    { kind: 'gate', name: 'streaming', required: true },
    {
      kind: 'event',
      runtime: 'main',
      event: { type: 'compaction_start', reason: 'threshold' },
    },
    { kind: 'gate', name: 'compacting', required: true },
    {
      kind: 'event',
      runtime: 'main',
      event: { type: 'compaction_end' },
    },
    { kind: 'event', runtime: 'main', event: { type: 'agent_settled' } },
    {
      kind: 'request',
      runtime: 'main',
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
      runtime: 'main',
      capture: 'settled-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'settled-efforts',
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: 'settled-messages',
      command: 'get_messages',
      data: { messages: history },
    },
    {
      kind: 'response',
      request: 'settled-efforts',
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
  ],
});

const reloadScenarios = [reloadResources, reloadRejected, reloadBusy];

export default reloadScenarios;
