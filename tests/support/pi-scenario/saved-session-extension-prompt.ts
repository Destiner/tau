import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario, type PiMessage, type PiScenarioStep } from './index';

const runtime = 'main';

/** How long the prompt stands before Pi stops waiting, in milliseconds. */
const promptTimeout = 5_000;

const history: readonly PiMessage[] = [
  { role: 'user', content: 'Open the release checklist' },
  {
    role: 'assistant',
    content: [{ type: 'text', text: 'Working through the checklist now.' }],
  },
];

/**
 * A saved session whose extension asks a question. The prompt takes the
 * composer's place inside the transcript, and expires on its own timeout so
 * that the composer coming back is covered without an answer in flight.
 */
const promptEvent = {
  kind: 'event',
  runtime,
  event: {
    type: 'extension_ui_request',
    id: 'prompt-1',
    method: 'select',
    title: 'Which label should the release carry?',
    message: 'Pick the one the changelog uses.',
    options: ['patch', 'minor', 'major'],
    timeout: promptTimeout,
  },
} as const satisfies PiScenarioStep;

const savedSessionExtensionPrompt = definePiScenario({
  metadata: {
    name: 'saved-session-extension-prompt',
    purpose:
      'Answer an extension prompt inside the transcript rather than the composer.',
    qualityRule:
      'docs/quality.md Locality principle: everything renders where its context lives.',
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
    promptEvent,
  ],
});

/**
 * The same question asked of a session with nothing in it yet, which is where a
 * workflow phase usually asks: the transcript holds the prompt alone, and the
 * empty session's full-height composer is not what takes the pane.
 */
const emptySessionExtensionPrompt = definePiScenario({
  metadata: {
    name: 'empty-session-extension-prompt',
    purpose: 'Ask an extension question in a session that holds no messages.',
    qualityRule:
      'docs/quality.md Locality principle: everything renders where its context lives.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
    promptEvent,
  ],
});

export default savedSessionExtensionPrompt;
export { emptySessionExtensionPrompt, promptTimeout };
