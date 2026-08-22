import archivedSessionsReview from './archived-sessions-review';
import phantomCommandRegistration from './phantom-command-registration';
import savedSessionCommandReplacement from './saved-session-command-replacement';
import savedSessionExtensionPrompt, {
  emptySessionExtensionPrompt,
} from './saved-session-extension-prompt';
import {
  savedSessionHistory,
  savedSessionLongHistory,
  savedSessionShortHistory,
} from './saved-session-history';
import {
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
} from './saved-session-process-failures';
import {
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
} from './saved-session-stream-then-stale-generation';
import savedSessionUnacknowledgedAbort from './saved-session-unacknowledged-abort';

import type { PiScenario, PiScenarioMetadata } from './index';

const scenarios = [
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
  savedSessionCommandReplacement,
  phantomCommandRegistration,
  savedSessionUnacknowledgedAbort,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
  savedSessionHistory,
  savedSessionShortHistory,
  savedSessionLongHistory,
  savedSessionExtensionPrompt,
  emptySessionExtensionPrompt,
  archivedSessionsReview,
] as const satisfies readonly PiScenario[];

const catalogue = new Map<string, PiScenario>(
  scenarios.map((scenario) => [scenario.metadata.name, scenario]),
);

function piScenarioCatalogue(): readonly PiScenarioMetadata[] {
  return scenarios.map((scenario) => scenario.metadata);
}

function findPiScenario(name: string): PiScenario | undefined {
  return catalogue.get(name);
}

export { findPiScenario, piScenarioCatalogue };
