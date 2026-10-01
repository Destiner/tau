import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import {
  definePiScenario,
  type PiMessage,
  type PiScenario,
  type PiScenarioStep,
} from './index';

const promptTimeout = 5_000;

const history: readonly PiMessage[] = [
  { role: 'user', content: 'Prepare the release notes' },
  {
    role: 'assistant',
    content: [{ type: 'text', text: 'The release notes need one label.' }],
  },
];

function textPrompt(
  name: string,
  method: 'input' | 'editor',
  title: string,
): PiScenario {
  const prompt = {
    kind: 'event',
    runtime: 'main',
    event: {
      type: 'extension_ui_request',
      id: `${method}-navigation-prompt`,
      method,
      title,
      message: 'Leave this answer unfinished while navigating.',
      placeholder: 'Type the release label',
      timeout: promptTimeout,
    },
  } as const satisfies PiScenarioStep;

  return definePiScenario({
    metadata: {
      name,
      purpose: `Preserve a pending extension ${method} draft while navigating.`,
      qualityRule:
        'docs/quality.md §§2 and 6: navigation preserves typed session-owned work and focus.',
      schemaVersion: 1,
    },
    runtimes: [{ key: 'main', generation: 2 }],
    steps: [
      ...successfulBootstrapSteps(
        'main',
        'main-bootstrap',
        mainSessionState,
      ).map((step) =>
        step.kind === 'response' && step.command === 'get_messages'
          ? { ...step, data: { messages: history } }
          : step,
      ),
      prompt,
    ],
  });
}

const savedSessionExtensionInputNavigation = textPrompt(
  'saved-session-extension-input-navigation',
  'input',
  'Release note label',
);

const savedSessionExtensionEditorNavigation = textPrompt(
  'saved-session-extension-editor-navigation',
  'editor',
  'Release notes',
);

export default savedSessionExtensionInputNavigation;
export {
  promptTimeout,
  savedSessionExtensionEditorNavigation,
  savedSessionExtensionInputNavigation,
};
