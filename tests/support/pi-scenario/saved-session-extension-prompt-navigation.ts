import {
  backupSessionState,
  mainSessionState,
  successfulBootstrapSteps,
} from './fixtures';

import { definePiScenario, type PiMessage, type PiScenarioStep } from './index';

const history: readonly PiMessage[] = [
  { role: 'user', content: 'Prepare the release notes' },
  {
    role: 'assistant',
    content: [{ type: 'text', text: 'The release notes need one label.' }],
  },
];

const navigationPrompt = {
  kind: 'event',
  runtime: 'main',
  event: {
    type: 'extension_ui_request',
    id: 'navigation-prompt',
    method: 'select',
    title: 'Which label should the release carry?',
    message: 'Leave this question unanswered while navigating.',
    options: ['patch', 'minor', 'major'],
  },
} as const satisfies PiScenarioStep;

const savedSessionExtensionPromptCycling = definePiScenario({
  metadata: {
    name: 'saved-session-extension-prompt-cycling',
    purpose: 'Cycle away from and back to a pending extension question.',
    qualityRule:
      'docs/quality.md §6: keyboard navigation returns focus to a valid control.',
    schemaVersion: 1,
  },
  runtimes: [
    { key: 'main', generation: 2 },
    { key: 'backup', generation: 2 },
  ],
  steps: [
    ...successfulBootstrapSteps('main', 'main-bootstrap', mainSessionState).map(
      (step) =>
        step.kind === 'response' && step.command === 'get_messages'
          ? { ...step, data: { messages: history } }
          : step,
    ),
    navigationPrompt,
    ...successfulBootstrapSteps(
      'backup',
      'backup-bootstrap',
      backupSessionState,
    ),
  ],
});

export default savedSessionExtensionPromptCycling;
