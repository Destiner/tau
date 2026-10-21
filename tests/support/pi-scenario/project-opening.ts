import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const targetProjectPath = '/fixture/target-project';
const targetSessionState = {
  ...mainSessionState,
  sessionId: 'session-target',
  sessionFile: `${targetProjectPath}/session-target.jsonl`,
  sessionName: 'Target',
} as const;

const projectOpening = definePiScenario({
  metadata: {
    name: 'project-opening',
    purpose:
      'Open an already-known local or remote project through the visible project controls.',
    qualityRule: 'State correctness and session locality',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'target', generation: 2 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'source', mainSessionState),
    ...successfulBootstrapSteps('target', 'target', targetSessionState),
  ],
});

export { targetProjectPath };
export default projectOpening;
