import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const savedSessionAutoRetry = definePiScenario({
  metadata: {
    name: 'saved-session-auto-retry',
    purpose:
      'Show and clear reviewed automatic-retry feedback without exposing provider payloads.',
    qualityRule: 'Failure copy, visual stability, and privacy',
    schemaVersion: 1,
  },
  runtimes: [{ key: 'main', generation: 1 }],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
    { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'retry-run-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'retry-run-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    { kind: 'gate', name: 'before-auto-retry', required: true },
    {
      kind: 'event',
      runtime: 'main',
      event: {
        type: 'auto_retry_start',
        attempt: 1,
        maxAttempts: 3,
        delayMs: 1_000,
        errorMessage: '429 RAW_RETRY_PROVIDER_PAYLOAD too many requests',
      },
    },
    { kind: 'gate', name: 'auto-retry-visible', required: true },
    {
      kind: 'event',
      runtime: 'main',
      event: { type: 'auto_retry_end', success: true, attempt: 1 },
    },
    { kind: 'gate', name: 'auto-retry-finished', required: true },
  ],
});

export default savedSessionAutoRetry;
