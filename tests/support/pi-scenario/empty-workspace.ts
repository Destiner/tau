import { definePiScenario } from './index';

const emptyWorkspace = definePiScenario({
  metadata: {
    name: 'empty-workspace',
    purpose: 'Open Tau with no projects or sessions.',
    qualityRule: 'Empty states are concise and offer a useful next step',
    schemaVersion: 1,
  },
  runtimes: [],
  steps: [],
});

export default emptyWorkspace;
