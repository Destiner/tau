import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

export default definePiScenario({
  metadata: {
    name: 'saved-session-queue-rejection',
    purpose:
      'A rejected queue submission preserves a newer composer draft and offers explicit recovery.',
    qualityRule:
      'docs/quality.md §2 Input integrity and §6 Keyboard and focus.',
    schemaVersion: 1,
  },
  runtimes: [{ key: 'main', generation: 2 }],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState),
    {
      kind: 'request',
      runtime: 'main',
      capture: 'initial-prompt',
      match: { type: 'prompt', message: 'Start held work' },
    },
    { kind: 'response', request: 'initial-prompt', command: 'prompt' },
    { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'working-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'working-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    { kind: 'gate', name: 'streaming-ready', required: true },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'steering-mode',
      match: { type: 'set_steering_mode', mode: 'all' },
    },
    {
      kind: 'response',
      request: 'steering-mode',
      command: 'set_steering_mode',
    },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'follow-up-mode',
      match: { type: 'set_follow_up_mode', mode: 'one-at-a-time' },
    },
    {
      kind: 'response',
      request: 'follow-up-mode',
      command: 'set_follow_up_mode',
    },
    {
      kind: 'request',
      runtime: 'main',
      capture: 'rejected-prompt',
      match: {
        type: 'prompt',
        message: 'Restore me later',
        streamingBehavior: 'steer',
      },
    },
    { kind: 'gate', name: 'submission-requested', required: true },
    {
      kind: 'response',
      request: 'rejected-prompt',
      command: 'prompt',
      success: false,
      error: 'Simulated rejection',
    },
    { kind: 'gate', name: 'submission-rejected', required: true },
  ],
});
