import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const targetProjectPath = '/fixture/target-project';
const targetRemoteDestination = { host: 'example', user: 'target', port: 2222 };
const targetRemoteLabel = 'target@example -p 2222';
const targetRemoteConnection = `ssh ${targetRemoteLabel}`;
const manualRemoteConnection = 'ssh manual-alias';
const manualRemoteDirectory = '/remote/manual';
const targetRemoteDirectory = '/remote/target-project';
const targetRemoteChild = `${targetRemoteDirectory}/child`;
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
      'Open an already-known local or remote project and exercise SSH history through the visible project controls.',
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

export {
  manualRemoteConnection,
  manualRemoteDirectory,
  targetProjectPath,
  targetRemoteChild,
  targetRemoteConnection,
  targetRemoteDestination,
  targetRemoteDirectory,
  targetRemoteLabel,
};
export default projectOpening;
