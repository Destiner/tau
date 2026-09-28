import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const longSentence =
  'Check the next tool result before making any further edits to this file';
const unbroken =
  'supercalifragilisticexpialidociousunbrokenidentifier123456789';
const multiline = '**Markdown**\nSecond line with more context';
const steering = ['Fix', longSentence, unbroken];
const followUp = [
  'Yes',
  longSentence,
  multiline,
  ...Array.from({ length: 8 }, (_, index) => `Later ${index + 4}`),
];

const savedSessionQueuePills = definePiScenario({
  metadata: {
    name: 'saved-session-queue-pills',
    purpose:
      'Render short, long, multiline, and double-digit queued messages and update their order.',
    qualityRule:
      'docs/quality.md §6 Keyboard and focus; §7 Visual stability and motion.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'main-bootstrap', mainSessionState),
    {
      kind: 'event',
      runtime,
      event: { type: 'queue_update', steering, followUp },
    },
    { kind: 'gate', name: 'pills-rendered', required: true },
    {
      kind: 'event',
      runtime,
      event: { type: 'queue_update', steering, followUp: followUp.slice(1) },
    },
    { kind: 'gate', name: 'pills-renumbered', required: true },
  ],
});

export { followUp, longSentence, multiline, steering, unbroken };
export default savedSessionQueuePills;
