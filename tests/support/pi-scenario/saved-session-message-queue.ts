import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const initialPrompt = 'Start a queueable fixture run';
const steeringMessage = 'Steer toward the focused assertion';
const followUpMessage = 'Summarize after the focused assertion';
const stopMessage = 'Keep this queued when stopping';

const savedSessionMessageQueue = definePiScenario({
  metadata: {
    name: 'saved-session-message-queue',
    purpose:
      'Queue steering and follow-up messages in a busy session without implicit aborts or clears.',
    qualityRule:
      'docs/quality.md §5 State correctness: queued work remains visible and intentional.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'main-bootstrap', mainSessionState),
    {
      kind: 'request',
      runtime,
      capture: 'initial-prompt',
      match: { type: 'prompt', message: initialPrompt },
    },
    { kind: 'response', request: 'initial-prompt', command: 'prompt' },
    { kind: 'event', runtime, event: { type: 'agent_start' } },
    {
      kind: 'request',
      runtime,
      capture: 'streaming-state',
      match: { type: 'get_state' },
    },
    {
      kind: 'response',
      request: 'streaming-state',
      command: 'get_state',
      data: { ...mainSessionState, isStreaming: true },
    },
    { kind: 'gate', name: 'streaming-ready', required: true },
    {
      kind: 'request',
      runtime,
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
      runtime,
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
      runtime,
      capture: 'steer-prompt',
      match: {
        type: 'prompt',
        message: steeringMessage,
        streamingBehavior: 'steer',
      },
    },
    { kind: 'response', request: 'steer-prompt', command: 'prompt' },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'queue_update',
        steering: [steeringMessage],
        followUp: [],
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'follow-up-prompt',
      match: {
        type: 'prompt',
        message: followUpMessage,
        streamingBehavior: 'followUp',
      },
    },
    { kind: 'response', request: 'follow-up-prompt', command: 'prompt' },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'queue_update',
        steering: [steeringMessage],
        followUp: [followUpMessage],
      },
    },
    { kind: 'gate', name: 'queue-rendered', required: true },
    {
      kind: 'request',
      runtime,
      capture: 'clear-queue',
      match: { type: 'clear_queue' },
    },
    { kind: 'gate', name: 'clear-requested', required: true },
    {
      kind: 'response',
      request: 'clear-queue',
      command: 'clear_queue',
      data: { steering: [], followUp: [] },
    },
    {
      kind: 'request',
      runtime,
      capture: 'stop-queue-prompt',
      match: {
        type: 'prompt',
        message: stopMessage,
        streamingBehavior: 'steer',
      },
    },
    { kind: 'response', request: 'stop-queue-prompt', command: 'prompt' },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'queue_update',
        steering: [stopMessage],
        followUp: [],
      },
    },
    { kind: 'gate', name: 'stop-ready', required: true },
    {
      kind: 'request',
      runtime,
      capture: 'abort',
      match: { type: 'abort' },
    },
    { kind: 'gate', name: 'stop-requested', required: true },
    { kind: 'runtime-event', runtime, event: 'exited', code: 0 },
  ],
});

export { followUpMessage, initialPrompt, steeringMessage, stopMessage };
export default savedSessionMessageQueue;
