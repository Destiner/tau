import archivedSessionsReview from './archived-sessions-review';
import commandListScrolling from './command-list-scrolling';
import crossProjectSidebarMetadataActions from './cross-project-sidebar-metadata-actions';
import delayedSuccessorVerification from './delayed-successor-verification';
import emptyWorkspace from './empty-workspace';
import {
  fixtureModels,
  fixtureThinkingLevels,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';
import paletteSettingsScenarios from './palette-settings';
import phantomCommandOnly from './phantom-command-only';
import phantomCommandRegistration from './phantom-command-registration';
import phantomCommandReplacement from './phantom-command-replacement';
import phantomFirstPromptRegistration from './phantom-first-prompt-registration';
import planImplementReplacement from './plan-implement-replacement';
import projectOpening from './project-opening';
import savedSessionAutoRetry from './saved-session-auto-retry';
import savedSessionCommandReplacement from './saved-session-command-replacement';
import savedSessionCompaction from './saved-session-compaction';
import savedSessionCompactionNotifications from './saved-session-compaction-notifications';
import compactionQueueScenarios, {
  compactionQueueNativeProfile,
} from './saved-session-compaction-queue';
import savedSessionExtensionPrompt, {
  emptySessionExtensionPrompt,
  escapeCancellationPrompt,
  savedSessionExtensionPromptProjectSwitching,
} from './saved-session-extension-prompt';
import savedSessionExtensionPromptCycling from './saved-session-extension-prompt-navigation';
import {
  savedSessionExtensionEditor,
  savedSessionExtensionEditorNavigation,
  savedSessionExtensionInput,
  savedSessionExtensionInputNavigation,
} from './saved-session-extension-text-prompt-navigation';
import {
  savedSessionHistory,
  savedSessionLongHistory,
  savedSessionShortHistory,
} from './saved-session-history';
import savedSessionMessageQueue from './saved-session-message-queue';
import savedSessionNoticeBoundaries from './saved-session-notice-boundaries';
import {
  remotePhantomPromptProcessExit,
  remoteSavedSessionProcessExit,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
} from './saved-session-process-failures';
import savedSessionPromptAdmission from './saved-session-prompt-admission';
import savedSessionQueuePills from './saved-session-queue-pills';
import savedSessionQueueRejection from './saved-session-queue-rejection';
import savedSessionSteeringBoundary from './saved-session-steering-boundary';
import {
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
} from './saved-session-stream-then-stale-generation';
import savedSessionUnacknowledgedAbort from './saved-session-unacknowledged-abort';

import {
  definePiScenario,
  type PiScenario,
  type PiScenarioMetadata,
  type PiScenarioStep,
} from './index';

const setupSkill = {
  name: 'skill:synthetic',
  description: 'Harmless synthetic setup regression skill',
  source: 'skill',
};
const fallbackModel = {
  provider: 'fixture',
  id: 'beta',
  name: 'Beta',
  reasoning: true,
};
const setupState = {
  ...mainSessionState,
  sessionId: 'session-usage',
  sessionFile: '/fixture/tau-project/session-usage.jsonl',
  sessionName: 'Synthetic setup',
};

function setupReadback(
  prefix: string,
  model: { provider: string; id: string; name: string },
): readonly PiScenarioStep[] {
  return [
    {
      kind: 'request',
      runtime: 'phantom',
      capture: `${prefix}-state`,
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: `${prefix}-state`,
      command: 'get_state',
      data: { ...setupState, model, isStreaming: false },
    },
    {
      kind: 'request',
      runtime: 'phantom',
      capture: `${prefix}-efforts`,
      match: { type: 'get_available_thinking_levels' },
    },
    {
      kind: 'response',
      request: `${prefix}-efforts`,
      command: 'get_available_thinking_levels',
      data: { levels: fixtureThinkingLevels },
    },
  ];
}

function initialSettingsFailure(picker: boolean): PiScenario {
  return definePiScenario({
    metadata: {
      name: picker
        ? 'initial-settings-failure-picker'
        : 'initial-settings-failure',
      purpose:
        'Preserve a synthetic skill draft after rejected inherited settings and require deliberate recovery and submission.',
      qualityRule:
        'docs/quality.md §2 Input integrity, §3 Failure, §5 State correctness, §6 Keyboard and focus.',
      schemaVersion: 1,
    },
    runtimes: [
      { key: 'main', generation: 2 },
      { key: 'phantom', generation: 1 },
    ],
    steps: [
      ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState, [
        setupSkill,
      ]),
      ...successfulBootstrapSteps(
        'phantom',
        'setup-bootstrap',
        { ...setupState, model: fallbackModel },
        [setupSkill],
      )
        .slice(0, 7)
        .map((step): PiScenarioStep =>
          step.kind === 'response' && step.command === 'get_available_models'
            ? { ...step, data: { models: [...fixtureModels, fallbackModel] } }
            : step,
        ),
      {
        kind: 'request',
        runtime: 'phantom',
        capture: 'initial-model',
        match: { type: 'set_model', provider: 'fixture', modelId: 'alpha' },
      },
      {
        kind: 'response',
        request: 'initial-model',
        command: 'set_model',
        success: false,
        error: 'Synthetic authentication failure PRIVATE_SETUP_CANARY',
      },
      ...setupReadback('rejected', fallbackModel),
      { kind: 'gate', name: 'setup-blocked', required: true },
      {
        kind: 'request',
        runtime: 'phantom',
        capture: 'recover-model',
        match: {
          type: 'set_model',
          provider: 'fixture',
          modelId: picker ? 'beta' : 'alpha',
        },
      },
      { kind: 'gate', name: 'setup-recovering', required: true },
      { kind: 'response', request: 'recover-model', command: 'set_model' },
      ...(!picker
        ? ([
            {
              kind: 'request',
              runtime: 'phantom',
              capture: 'recover-effort',
              match: { type: 'set_thinking_level', level: 'high' },
            },
            {
              kind: 'response',
              request: 'recover-effort',
              command: 'set_thinking_level',
            },
          ] satisfies PiScenarioStep[])
        : []),
      ...setupReadback(
        'recovered',
        picker ? fallbackModel : mainSessionState.model,
      ),
      { kind: 'gate', name: 'setup-ready', required: true },
      {
        kind: 'request',
        runtime: 'phantom',
        capture: 'deliberate-prompt',
        match: {
          type: 'prompt',
          message: '/skill:synthetic edited during recovery',
        },
      },
      { kind: 'response', request: 'deliberate-prompt', command: 'prompt' },
    ],
  });
}

const initialSettingsFailureScenarios = [
  initialSettingsFailure(false),
  initialSettingsFailure(true),
];

const scenarios = [
  emptyWorkspace,
  ...initialSettingsFailureScenarios,
  ...paletteSettingsScenarios,
  commandListScrolling,
  crossProjectSidebarMetadataActions,
  delayedSuccessorVerification,
  savedSessionBootstrap,
  savedSessionAutoRetry,
  savedSessionConversation,
  savedSessionCompaction,
  savedSessionCompactionNotifications,
  savedSessionNoticeBoundaries,
  ...compactionQueueScenarios,
  savedSessionPromptAdmission,
  savedSessionMessageQueue,
  savedSessionQueuePills,
  savedSessionQueueRejection,
  savedSessionSteeringBoundary,
  savedSessionStaleGeneration,
  savedSessionCommandReplacement,
  planImplementReplacement,
  projectOpening,
  phantomCommandRegistration,
  phantomCommandReplacement,
  phantomFirstPromptRegistration,
  phantomCommandOnly,
  savedSessionUnacknowledgedAbort,
  remotePhantomPromptProcessExit,
  remoteSavedSessionProcessExit,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
  savedSessionHistory,
  savedSessionShortHistory,
  savedSessionLongHistory,
  savedSessionExtensionPrompt,
  savedSessionExtensionPromptProjectSwitching,
  savedSessionExtensionPromptCycling,
  savedSessionExtensionInput,
  savedSessionExtensionEditor,
  savedSessionExtensionInputNavigation,
  savedSessionExtensionEditorNavigation,
  emptySessionExtensionPrompt,
  escapeCancellationPrompt,
  archivedSessionsReview,
] as const satisfies readonly PiScenario[];

const catalogue = new Map<string, PiScenario>(
  scenarios.map((scenario) => [scenario.metadata.name, scenario]),
);

function piScenarioCatalogue(): readonly PiScenarioMetadata[] {
  return scenarios.map((scenario) => scenario.metadata);
}

function findPiScenario(name: string): PiScenario | undefined {
  // The browser adapter's native fixtures are keyed by established scenario
  // names. Queue cases vary the RPC script, not those native fixtures.
  const location = (globalThis as { location?: { search: string } }).location;
  const variant = new URLSearchParams(location?.search).get(
    'test-scenario-variant',
  );
  const setupScenario = initialSettingsFailureScenarios.find(
    (scenario) =>
      scenario.metadata.name === variant || scenario.metadata.name === name,
  );
  if (setupScenario) {
    // Reuse the disposable phantom session's native fixture; only the RPC tape differs.
    if (
      name !== 'phantom-command-only' &&
      name !== setupScenario.metadata.name
    ) {
      throw new Error(
        `Setup scenario ${variant} requires native profile phantom-command-only.`,
      );
    }
    return setupScenario;
  }
  const queueScenario = compactionQueueScenarios.find(
    (scenario) => scenario.metadata.name === variant,
  );
  if (queueScenario) {
    const profile = compactionQueueNativeProfile(queueScenario.metadata.name);
    if (name !== profile) {
      throw new Error(
        `Queue scenario ${variant} requires native profile ${profile}.`,
      );
    }
    return queueScenario;
  }
  return catalogue.get(name);
}

export { findPiScenario, piScenarioCatalogue };
