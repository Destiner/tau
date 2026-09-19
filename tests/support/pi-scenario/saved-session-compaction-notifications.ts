import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario } from './index';

const runtime = 'main';
const runtimes = [{ key: runtime, generation: 2 }] as const;

const notify = (
  id: string,
  message: string,
  notifyType: 'info' | 'warning' | 'error',
) =>
  ({
    kind: 'event',
    runtime,
    event: {
      type: 'extension_ui_request',
      id,
      method: 'notify',
      message,
      notifyType,
    },
  }) as const;

const scenario = definePiScenario({
  metadata: {
    name: 'saved-session-compaction-notifications',
    purpose:
      'Clear pre-compaction local feedback while preserving the order of notifications raised during compacted hydration.',
    qualityRule: 'State correctness and transcript continuity',
    schemaVersion: 1,
  },
  runtimes,
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
    notify('old-info', 'Old informational notice', 'info'),
    notify('old-warning', 'Old warning notice', 'warning'),
    notify('old-error', 'Old error notice', 'error'),
    {
      kind: 'event',
      runtime,
      event: {
        type: 'compaction_end',
        errorMessage: 'Fixture compaction failure',
      },
    },
    { kind: 'gate', name: 'before-successful-compaction', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'compaction_end',
        result: { summary: 'Compacted fixture context.' },
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'compacted-messages',
      match: { type: 'get_messages' },
    },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Output before fresh notice.',
        },
      },
    },
    notify('fresh-warning', 'Fresh warning notice', 'warning'),
    {
      kind: 'event',
      runtime,
      event: {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'Output after fresh notice.',
        },
      },
    },
    { kind: 'gate', name: 'compacted-hydration-pending', required: true },
    {
      kind: 'response',
      request: 'compacted-messages',
      command: 'get_messages',
      data: {
        messages: [
          {
            role: 'compactionSummary',
            summary: 'Compacted fixture context.',
          },
          {
            role: 'assistant',
            content: [
              {
                type: 'text',
                text: 'Output before fresh notice.Output after fresh notice.',
              },
            ],
          },
        ],
      },
    },
    {
      kind: 'gate',
      name: 'compacted-notifications-reconciled',
      required: true,
    },
  ],
});

export default scenario;
