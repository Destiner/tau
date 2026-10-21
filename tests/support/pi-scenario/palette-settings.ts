import {
  backupSessionState,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import {
  definePiScenario,
  type PiScenario,
  type PiScenarioStep,
  type PiState,
} from './index';

const models = [
  { provider: 'first', id: 'shared', name: 'Alpha Shared', reasoning: true },
  { provider: 'second', id: 'shared', name: 'Beta Shared', reasoning: true },
  { provider: 'third', id: 'gamma', name: 'Gamma', reasoning: true },
];
const currentState = { ...mainSessionState, model: models[1]! };

function bootstrap(
  runtime: string,
  state: Omit<PiState, 'isStreaming'>,
  availableModels = models,
  levels = ['low', 'high', 'off'],
): readonly PiScenarioStep[] {
  return successfulBootstrapSteps(runtime, runtime, state).map((step) => {
    if (step.kind === 'response' && step.command === 'get_available_models') {
      return { ...step, data: { models: availableModels } };
    }
    if (
      step.kind === 'response' &&
      step.command === 'get_available_thinking_levels'
    ) {
      return { ...step, data: { levels } };
    }
    return step;
  });
}

function scenario(
  name: string,
  steps: readonly PiScenarioStep[],
  backup = false,
): PiScenario {
  return definePiScenario({
    metadata: {
      name,
      purpose:
        'Verify palette candidates initialize from session settings without applying unconfirmed choices.',
      qualityRule:
        'docs/quality.md §2 Input integrity, §5 State correctness, §6 Keyboard and focus.',
      schemaVersion: 1,
    },
    runtimes: [
      { key: 'main', generation: 2 },
      ...(backup ? [{ key: 'backup', generation: 2 }] : []),
    ],
    steps,
  });
}

const paletteSettings = scenario(
  'palette-settings',
  bootstrap('main', currentState),
);
const paletteSettingsOff = scenario(
  'palette-settings-off',
  bootstrap('main', { ...currentState, thinkingLevel: 'off' }),
);
const paletteSettingsUnavailable = scenario(
  'palette-settings-unavailable',
  bootstrap('main', {
    ...currentState,
    model: { provider: 'missing', id: 'missing', name: 'Unavailable' },
    thinkingLevel: 'medium',
  }),
);
const paletteSettingsEmpty = scenario(
  'palette-settings-empty',
  bootstrap('main', currentState, [], []),
);
const paletteSettingsSessions = scenario(
  'palette-settings-sessions',
  [
    ...bootstrap('main', currentState),
    ...bootstrap('backup', {
      ...backupSessionState,
      model: models[2]!,
      thinkingLevel: 'off',
    }),
  ],
  true,
);
const paletteSettingsConfirm = scenario('palette-settings-confirm', [
  ...bootstrap('main', currentState),
  {
    kind: 'request',
    runtime: 'main',
    capture: 'change-model',
    match: { type: 'set_model', provider: 'first', modelId: 'shared' },
  },
  { kind: 'response', request: 'change-model', command: 'set_model' },
  {
    kind: 'request',
    runtime: 'main',
    capture: 'changed-state',
    match: { type: 'get_state' },
  },
  {
    kind: 'response',
    request: 'changed-state',
    command: 'get_state',
    data: { ...currentState, model: models[0]!, isStreaming: false },
  },
  {
    kind: 'request',
    runtime: 'main',
    capture: 'changed-messages',
    match: { type: 'get_messages' },
  },
  {
    kind: 'request',
    runtime: 'main',
    capture: 'changed-efforts',
    match: { type: 'get_available_thinking_levels' },
  },
  {
    kind: 'response',
    request: 'changed-efforts',
    command: 'get_available_thinking_levels',
    data: { levels: ['low', 'high', 'off'] },
  },
  {
    kind: 'response',
    request: 'changed-messages',
    command: 'get_messages',
    data: { messages: [] },
  },
  {
    kind: 'request',
    runtime: 'main',
    capture: 'change-effort',
    match: { type: 'set_thinking_level', level: 'off' },
  },
  { kind: 'response', request: 'change-effort', command: 'set_thinking_level' },
]);

const paletteSettingsScenarios = [
  paletteSettings,
  paletteSettingsOff,
  paletteSettingsUnavailable,
  paletteSettingsEmpty,
  paletteSettingsSessions,
  paletteSettingsConfirm,
];

export default paletteSettingsScenarios;
