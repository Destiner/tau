import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario, type PiMessage } from './index';

const runtime = 'main';
const requestId = 'release-label-select';
const history: readonly PiMessage[] = [
  { role: 'user', content: 'Review the release notes' },
];

const savedSessionExtensionSelectWhileTyping = definePiScenario({
  metadata: {
    name: 'saved-session-extension-select-while-typing',
    purpose:
      'Keep a Pi select prompt open when it arrives mid-draft and answer it with the keyboard.',
    qualityRule:
      'docs/quality.md §2 Input integrity and §6 Keyboard and focus.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState).map(
      (step) =>
        step.kind === 'response' && step.command === 'get_messages'
          ? { ...step, data: { messages: history } }
          : step,
    ),
    { kind: 'gate', name: 'before-extension-select', required: true },
    {
      kind: 'event',
      runtime,
      event: {
        type: 'extension_ui_request',
        id: requestId,
        method: 'select',
        title: 'Which release label should we use?',
        options: ['patch', 'minor', 'major'],
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'selected-release-label',
      match: {
        type: 'extension_ui_response',
        extensionRequestId: requestId,
        variant: 'value',
        value: 'minor',
      },
    },
  ],
});

export default savedSessionExtensionSelectWhileTyping;
