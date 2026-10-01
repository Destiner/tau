import { mainSessionState, successfulBootstrapSteps } from './fixtures';

import { definePiScenario, type PiScenarioStep } from './index';

const runtime = 'main';

/**
 * Keep an extension editor owned by Main while sidebar actions mutate idle
 * rows in this and another project. The response must not follow sidebar
 * focus or either archived row into a different runtime.
 */
const crossProjectSidebarMetadataActions = definePiScenario({
  metadata: {
    name: 'cross-project-sidebar-metadata-actions',
    purpose:
      'Keep an active extension editor attached to Main while archiving idle sidebar sessions across projects.',
    qualityRule:
      'Session isolation: sidebar metadata actions must preserve the selected runtime and its extension UI.',
    schemaVersion: 1,
  },
  runtimes: [{ key: runtime, generation: 2 }],
  steps: [
    ...successfulBootstrapSteps(runtime, 'bootstrap', mainSessionState),
    {
      kind: 'event',
      runtime,
      event: {
        type: 'extension_ui_request',
        id: 'main-sidebar-metadata-editor',
        method: 'editor',
        title: 'Describe the metadata change',
        message: 'This editor remains owned by Main.',
        placeholder: 'Add the session note',
        prefill: 'Keep Main selected while archiving idle sessions.',
      },
    },
    {
      kind: 'request',
      runtime,
      capture: 'main-sidebar-metadata-editor-response',
      match: {
        type: 'extension_ui_response',
        extensionRequestId: 'main-sidebar-metadata-editor',
        variant: 'value',
        value: 'Archive the selected sidebar metadata.',
      },
    },
  ] satisfies readonly PiScenarioStep[],
});

export default crossProjectSidebarMetadataActions;
