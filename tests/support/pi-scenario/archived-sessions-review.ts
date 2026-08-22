import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';

/**
 * A workspace holding an archived session alongside a live one, so the
 * archived-sessions view — the footer toggle, time groups, and the unarchive
 * action — can be exercised without touching real session storage.
 */
const archivedSessionsReview = definePiScenario({
  metadata: {
    name: 'archived-sessions-review',
    purpose: 'Review the archived-sessions view and unarchive a session.',
    qualityRule: 'Session list correctness',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
});

export default archivedSessionsReview;
