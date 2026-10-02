import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const commands = Array.from({ length: 18 }, (_, index) => ({
  name: `scroll-command-${String(index).padStart(2, '0')}`,
  ...(index % 2 === 0
    ? { description: `Deterministic scrolling command ${index}` }
    : {}),
  source: 'extension',
}));

const commandListScrolling = definePiScenario({
  metadata: {
    name: 'command-list-scrolling',
    purpose:
      'Expose an overflowing, mixed-height slash-command list for browser scrolling regressions.',
    qualityRule:
      'docs/quality.md §4 Input integrity: native scrolling and keyboard navigation remain predictable.',
    schemaVersion: 1,
  },
  runtimes: [{ key: 'main', generation: 2 }],
  steps: successfulBootstrapSteps(
    'main',
    'bootstrap',
    mainSessionState,
    commands,
  ),
});

export default commandListScrolling;
