import savedSessionCommandReplacement from './saved-session-command-replacement';
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
  savedSessionUnacknowledgedAbort,
  savedSessionBootstrapProcessExit,
  savedSessionPromptProcessExit,
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
