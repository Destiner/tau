import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { errorCopy } from '../../lib/error-copy';
import type { PiBridgeEvent } from '../../lib/pi/bridge';
import { frontendOwnership, OwnershipClaimError } from '../../lib/pi/ownership';
import {
  clearAbortWatch,
  clearExtensionUiState,
  clearRemoteConnectionWatch,
  clearSessionReplacementWatch,
  clearSettingRequestWatch,
  handleBridgeEvent,
  releaseOrCacheRuntime,
  startController,
  stopControllerProcess,
} from '../../lib/pi/runtime';
import { invokeTraced } from '../../lib/telemetry';
import {
  activeController,
  controllerByRuntimeId,
  ensureController,
  projectSessions,
  setActiveSessionView,
  setControllerError,
  setControllerLifecycle,
  setWorkspaceError,
  state,
  type ProjectSummary,
  type SessionController,
  type WorkspaceSnapshot,
} from '../state';

import {
  currentLifecycle,
  invalidateLifecycle,
  readSavedSession,
  cancelSelectionTracking,
  cancelSavedPreview,
} from './navigation';

interface InitializationOperation {
  lifecycle: number;
  promise: Promise<void>;
}

let unlisten: UnlistenFn | undefined;
let initialization: InitializationOperation | undefined;

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function createLifecycle(
  newSession: (
    project: ProjectSummary,
    bootstrap?: boolean,
  ) => Promise<SessionController | undefined>,
) {
  function initialize(): Promise<void> {
    if (initialization) {
      initialization.lifecycle = currentLifecycle();
      state.initializing = true;
      return initialization.promise;
    }
    state.initializing = true;
    state.ownershipFailure = null;
    const operation: InitializationOperation = {
      lifecycle: currentLifecycle(),
      promise: Promise.resolve(),
    };
    operation.promise = initializeOnce(operation)
      .catch(() => {
        if (operation.lifecycle === currentLifecycle()) {
          setWorkspaceError(errorCopy.piOwnership);
          state.ownershipFailure = 'retryable';
        }
      })
      .finally(() => {
        if (initialization === operation) initialization = undefined;
        if (operation.lifecycle === currentLifecycle()) {
          state.initializing = false;
        } else {
          unlisten?.();
          unlisten = undefined;
        }
      });
    initialization = operation;
    return operation.promise;
  }

  async function initializeOnce(
    operation: InitializationOperation,
  ): Promise<void> {
    if (!unlisten) {
      const nextUnlisten = await listen<PiBridgeEvent>(
        'pi-event',
        ({ payload }) => {
          void handleBridgeEvent(payload).catch(() => {
            const controller = controllerByRuntimeId(payload.runtimeId);
            if (!controller || controller.generation !== payload.generation) {
              return;
            }
            setControllerLifecycle(
              controller,
              { syncing: false },
              'bridge_event_failed',
            );
            setControllerError(controller, errorCopy.bridgeEvent);
            releaseOrCacheRuntime(controller);
          });
        },
      );
      if (operation.lifecycle !== currentLifecycle()) {
        nextUnlisten();
        return;
      }
      unlisten = nextUnlisten;
    }

    try {
      await frontendOwnership.claim();
    } catch (error) {
      if (operation.lifecycle === currentLifecycle()) {
        const conflict =
          error instanceof OwnershipClaimError && error.kind === 'conflict';
        state.ownershipFailure = conflict ? 'conflict' : 'retryable';
        setWorkspaceError(
          conflict ? errorCopy.piOwnershipConflict : errorCopy.piOwnership,
        );
      }
      return;
    }
    if (operation.lifecycle !== currentLifecycle()) return;

    try {
      const workspace = await invokeTraced<WorkspaceSnapshot>(
        'load_workspace',
        {},
      );
      if (operation.lifecycle !== currentLifecycle()) return;
      state.workspace = workspace;
      state.activeProjectPath = state.workspace.activeProjectPath;
      const selectedProject = state.workspace.projects.find(
        (project) => project.selected,
      );
      const selectedSession = selectedProject?.sessions.find(
        (session) => session.selected,
      );
      if (
        selectedProject &&
        !selectedSession &&
        projectSessions(selectedProject).length === 0
      ) {
        const controller = await newSession(selectedProject, true);
        if (operation.lifecycle !== currentLifecycle() && controller) {
          await stopControllerProcess(controller, undefined, false);
        }
      } else if (selectedProject && selectedSession) {
        const controller = ensureController(selectedProject, selectedSession);
        setActiveSessionView(selectedProject, selectedSession, controller);
        const needsLocalRuntime = !selectedProject.connectionString;
        if (needsLocalRuntime && !state.workspace.piPath) {
          setControllerError(controller, errorCopy.piNotFound);
          return;
        }
        readSavedSession(selectedProject, selectedSession, controller);
        await startController(
          controller,
          selectedProject,
          selectedSession.path,
        );
        if (operation.lifecycle !== currentLifecycle()) {
          await stopControllerProcess(controller, undefined, false);
        }
      }
    } catch {
      const controller = activeController.value;
      if (controller) {
        setControllerLifecycle(
          controller,
          { starting: false },
          'workspace_load_failed',
        );
        setControllerError(controller, errorCopy.loadWorkspace);
      } else {
        setWorkspaceError(errorCopy.loadWorkspace);
      }
    }
  }

  function dispose(): void {
    cancelSelectionTracking();
    cancelSavedPreview();
    invalidateLifecycle();
    if (!initialization) state.initializing = false;
    if (!initialization) {
      unlisten?.();
      unlisten = undefined;
    }
    for (const controller of state.controllers) {
      controller.retry = undefined;
      clearSessionReplacementWatch(controller);
      clearAbortWatch(controller);
      clearRemoteConnectionWatch(controller);
      clearSettingRequestWatch(controller);
    }
    clearExtensionUiState();
  }
  return { initialize, dispose };
}

export default createLifecycle;
