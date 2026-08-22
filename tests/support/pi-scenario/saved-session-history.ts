import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario, type PiMessage, type PiScenarioStep } from './index';

const runtime = 'main';

/**
 * A saved session that already holds a transcript when it is opened. Every
 * other scenario bootstraps into an empty session, which mounts the composer
 * rather than the transcript, so nothing else covers the first render of a
 * transcript that arrives whole.
 */
function historyMessages(turns: number): readonly PiMessage[] {
  const messages: PiMessage[] = [];
  for (let turn = 0; turn < turns; turn += 1) {
    messages.push({ role: 'user', content: `History prompt ${turn}` });
    messages.push({
      role: 'assistant',
      content: [
        {
          type: 'text',
          text:
            turn === 0
              ? "I'll start by reading the issue."
              : `Reply ${turn}. ${'Another sentence of history. '.repeat(turn % 5)}`,
        },
      ],
    });
  }
  return messages;
}

function historySteps(turns: number): readonly PiScenarioStep[] {
  return successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState).map(
    (step) =>
      step.kind === 'response' && step.command === 'get_messages'
        ? { ...step, data: { messages: historyMessages(turns) } }
        : step,
  );
}

const savedSessionHistory = definePiScenario({
  metadata: {
    name: 'saved-session-history',
    purpose: 'Open a saved session whose transcript arrives already complete.',
    qualityRule: 'Transcript render correctness',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: historySteps(12),
});

const savedSessionShortHistory = definePiScenario({
  metadata: {
    name: 'saved-session-short-history',
    purpose:
      'Open a saved session holding one turn, too short to fill the viewport.',
    qualityRule: 'Transcript render correctness',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: historySteps(1),
});

const savedSessionLongHistory = definePiScenario({
  metadata: {
    name: 'saved-session-long-history',
    purpose: 'Open a saved session holding a long transcript.',
    qualityRule: 'Transcript render correctness',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: historySteps(60),
});

export {
  savedSessionHistory,
  savedSessionLongHistory,
  savedSessionShortHistory,
};
