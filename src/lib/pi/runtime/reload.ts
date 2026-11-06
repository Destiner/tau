import {
  nextRequestId,
  setControllerError,
  setControllerLifecycle,
  state,
  type SessionController,
} from '../../../composables/state';
import {
  TAU_RELOAD_COMMAND,
  reloadHelperAvailable,
} from '../extension-command';

import {
  rpc,
  releaseIdleRuntimes,
  stopControllerProcess,
  startController,
} from './lifecycle';
import { restoreSubmittedDraft } from './prompts';

type ReloadRead =
  | 'get_state'
  | 'get_commands'
  | 'get_available_models'
  | 'get_available_thinking_levels'
  | 'get_messages';
interface ReloadOperation {
  generation: number;
  requestId: string;
  draft: string;
  phase: 'discovery' | 'dispatch' | 'refresh';
  status: 'reloading' | 'unknown';
  reads: Record<string, ReloadRead>;
  failed: boolean;
  restored?: boolean;
  setupFailure?: SessionController['setupFailure'];
  timer?: ReturnType<typeof setTimeout>;
}

function clearReload(
  controller: SessionController,
  recoverDraft = false,
): void {
  if (recoverDraft && controller.reload) restore(controller, controller.reload);
  if (controller.reload?.timer) clearTimeout(controller.reload.timer);
  controller.reload = undefined;
}

function restore(
  controller: SessionController,
  operation: ReloadOperation,
): void {
  if (operation.restored) return;
  operation.restored = true;
  restoreSubmittedDraft(controller, operation.draft);
}

function fail(controller: SessionController, message: string): void {
  const operation = controller.reload;
  if (!operation) return;
  restore(controller, operation);
  clearReload(controller);
  setControllerError(controller, message);
  releaseIdleRuntimes();
}

function watch(controller: SessionController, milliseconds: number): void {
  const operation = controller.reload;
  if (!operation) return;
  if (operation.timer) clearTimeout(operation.timer);
  operation.timer = setTimeout(() => {
    if (
      controller.reload !== operation ||
      controller.disposed ||
      controller.generation !== operation.generation
    )
      return;
    // Supported extension input is an intentional wait, not a hung hook.
    if (
      state.extensionDialogs.some(
        (dialog) => dialog.controllerKey === controller.key,
      )
    ) {
      watch(controller, milliseconds);
      return;
    }
    if (operation.phase === 'discovery') {
      fail(
        controller,
        'Pi reload helper discovery timed out. Recover the runtime and try again.',
      );
      return;
    }
    operation.status = 'unknown';
    restore(controller, operation);
    setControllerError(
      controller,
      'Reload outcome is unknown. Wait for Pi to finish or recover the runtime before making changes.',
    );
  }, milliseconds);
}

function resumeReloadDeadline(controller: SessionController): void {
  const operation = controller.reload;
  if (!operation || operation.status === 'unknown') return;
  watch(controller, operation.phase === 'refresh' ? 10_000 : 30_000);
}

async function read(
  controller: SessionController,
  method: ReloadRead,
): Promise<void> {
  const operation = controller.reload;
  if (!operation) return;
  const id = nextRequestId('reload-read');
  operation.reads[id] = method;
  controller.reloadFence ??= {};
  controller.reloadFence[method] = Number(id.split('-').at(-1));
  try {
    await rpc(controller, { id, type: method });
  } catch {
    if (controller.reload !== operation) return;
    if (operation.phase === 'discovery') {
      fail(
        controller,
        'Pi reload helper discovery failed. Recover the runtime and try again.',
      );
      return;
    }
    operation.status = 'unknown';
    restore(controller, operation);
    setControllerError(
      controller,
      'Pi resources could not be refreshed. Recover the runtime before making changes.',
    );
  }
}

async function dispatchReload(controller: SessionController): Promise<void> {
  const operation = controller.reload;
  if (
    !operation ||
    operation.phase !== 'discovery' ||
    !controller.commandsLoaded
  )
    return;
  if (!reloadHelperAvailable(controller)) {
    fail(
      controller,
      'Pi reload helper is missing or ambiguous. Update Pi and recover the Tau runtime.',
    );
    return;
  }
  if (controller.streaming || controller.compacting || controller.stopping) {
    fail(
      controller,
      'Wait for Pi to finish responding or compacting before reloading.',
    );
    return;
  }
  operation.phase = 'dispatch';
  operation.requestId = nextRequestId('reload');
  watch(controller, 30_000);
  try {
    await rpc(controller, {
      id: operation.requestId,
      type: 'prompt',
      message: `/${TAU_RELOAD_COMMAND}`,
    });
  } catch {
    if (controller.reload === operation && operation.phase === 'dispatch') {
      operation.status = 'unknown';
      restore(controller, operation);
      setControllerError(
        controller,
        'Reload outcome is unknown. Wait for Pi to finish or recover the runtime before making changes.',
      );
    }
  }
}

async function recoverReload(
  controller: SessionController | undefined,
): Promise<void> {
  if (
    !controller ||
    controller.reload?.status !== 'unknown' ||
    controller.stopping
  )
    return;
  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project) return;
  controller.stopping = true;
  await stopControllerProcess(controller, undefined, false);
  if (controller.generation || controller.disposed) {
    controller.stopping = false;
    return;
  }
  await startController(
    controller,
    project,
    controller.sessionPath || undefined,
    true,
  );
}

async function startReload(
  controller: SessionController,
  draft: string,
): Promise<void> {
  if (controller.reload) return;
  controller.reload = {
    generation: controller.generation,
    requestId: '',
    draft,
    phase: 'discovery',
    status: 'reloading',
    reads: {},
    failed: false,
    setupFailure: controller.setupFailure,
  };
  controller.promptSubmitting = false;
  setControllerLifecycle(
    controller,
    { starting: false, working: false },
    'pending_prompt_dispatch',
  );
  watch(controller, 30_000);
  if (controller.commandsLoaded) await dispatchReload(controller);
  else await read(controller, 'get_commands');
}

// Called before ordinary response reduction; only this operation's exact reads
// may change catalogues once reload starts. The fence survives completion.
function reloadAllowsResponse(
  controller: SessionController,
  response: Record<string, unknown>,
): boolean {
  const method = String(response.command);
  if (
    ![
      'get_commands',
      'get_available_models',
      'get_available_thinking_levels',
      'get_messages',
      'get_state',
    ].includes(method)
  )
    return true;
  if (
    controller.reloadFence?.[method] &&
    Number(String(response.id).split('-').at(-1)) <
      controller.reloadFence[method]
  )
    return false;
  return true;
}

async function handleReloadResponse(
  controller: SessionController,
  response: Record<string, unknown>,
  after = false,
): Promise<boolean> {
  const operation = controller.reload;
  if (!operation || operation.generation !== controller.generation)
    return false;
  const id = String(response.id);
  if (id === operation.requestId && !after) {
    if (operation.phase !== 'dispatch') return true;
    if (
      response.success !== true ||
      (response.data as { disposition?: string } | undefined)?.disposition ===
        'rejected'
    ) {
      fail(
        controller,
        'Pi rejected the resource reload. Your command was restored.',
      );
      return true;
    }
    operation.phase = 'refresh';
    watch(controller, 10_000);
    await read(controller, 'get_state');
    return true;
  }
  const method = operation.reads[id];
  if (!method || !after) return false;
  delete operation.reads[id];
  if (response.success !== true) {
    if (operation.phase === 'discovery') {
      fail(
        controller,
        'Pi reload helper discovery failed. Recover the runtime and try again.',
      );
      return false;
    }
    operation.failed = true;
    operation.status = 'unknown';
    restore(controller, operation);
    setControllerError(
      controller,
      'Pi resources could not be refreshed. Recover the runtime before making changes.',
    );
    return false;
  }
  if (operation.phase === 'discovery') {
    await dispatchReload(controller);
    return false;
  }
  if (method === 'get_state') {
    await Promise.all(
      (
        [
          'get_commands',
          'get_available_models',
          'get_available_thinking_levels',
          'get_messages',
        ] as const
      ).map((resource) => read(controller, resource)),
    );
  }
  if (!Object.keys(operation.reads).length && !operation.failed) {
    clearReload(controller);
    releaseIdleRuntimes();
  }
  return false;
}

function reloadExtensionError(controller: SessionController): void {
  const operation = controller.reload;
  if (!operation) return;
  // Reload hooks may fail under any extension name, not just the helper.
  restore(controller, operation);
  setControllerError(
    controller,
    'Pi reported an extension error while reloading resources.',
  );
}

export {
  clearReload,
  startReload,
  recoverReload,
  reloadAllowsResponse,
  handleReloadResponse,
  reloadExtensionError,
  resumeReloadDeadline,
  type ReloadOperation,
};
