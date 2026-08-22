import { successfulBootstrapSteps } from './fixtures';

import { definePiScenario, type PiScenarioStep } from './index';

const runtime = 'main';
const archivedRuntime = 'archived';

const archivedSessionState = {
  sessionId: 'session-archived',
  sessionFile: '/fixture/tau-project/session-archived.jsonl',
  sessionName: 'Older archived work',
  model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
  thinkingLevel: 'high',
} as const;

/**
 * A workspace holding an archived session alongside a live one, so the
 * archived-sessions view — the footer toggle, time groups, opening a session
 * read-only, and unarchiving — can be exercised without touching real session
 * storage. The archived session bootstraps into its own runtime when opened.
 */
const archivedSessionsReview = definePiScenario({
  metadata: {
    name: 'archived-sessions-review',
    purpose:
      'Review the archived-sessions view and open or unarchive a session.',
    qualityRule: 'Session list correctness',
    schemaVersion: 1,
  },
  runtimes: [
    { key: runtime, generation: 2 },
    { key: archivedRuntime, generation: 1 },
  ],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', {
      sessionId: 'session-main',
      sessionFile: '/fixture/tau-project/session-main.jsonl',
      sessionName: 'Main',
      model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
      thinkingLevel: 'high',
    }),
    ...successfulBootstrapSteps(archivedRuntime, 'archived-bootstrap', {
      ...archivedSessionState,
    }),
  ] satisfies readonly PiScenarioStep[],
});

export default archivedSessionsReview;
