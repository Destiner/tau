import archivedSessionsReview from './archived-sessions-review';
import emptyWorkspace from './empty-workspace';
import phantomCommandOnly from './phantom-command-only';
import phantomCommandRegistration from './phantom-command-registration';
import planImplementReplacement from './plan-implement-replacement';
import savedSessionCommandReplacement from './saved-session-command-replacement';
import savedSessionCompaction from './saved-session-compaction';
import savedSessionExtensionPrompt, {
  emptySessionExtensionPrompt,
} from './saved-session-extension-prompt';
import {
  savedSessionHistory,
  savedSessionLongHistory,
  savedSessionShortHistory,
} from './saved-session-history';
import {
  remotePhantomPromptProcessExit,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
} from './saved-session-process-failures';
import savedSessionPromptAdmission from './saved-session-prompt-admission';
import {
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
} from './saved-session-stream-then-stale-generation';
import savedSessionUnacknowledgedAbort from './saved-session-unacknowledged-abort';

import type { PiScenario, PiScenarioMetadata } from './index';

const scenarios = [
  emptyWorkspace,
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionCompaction,
  savedSessionPromptAdmission,
  savedSessionStaleGeneration,
  savedSessionCommandReplacement,
  planImplementReplacement,
  phantomCommandRegistration,
  phantomCommandOnly,
  savedSessionUnacknowledgedAbort,
  remotePhantomPromptProcessExit,
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
