import { invoke } from '@tauri-apps/api/core';
import { nextTick, watch, type WatchStopHandle } from 'vue';

import { hydrateTranscript } from '../../lib/pi/transcript';
import { startActionMilestones } from '../../lib/telemetry';
import {
  state,
  type ProjectSummary,
  type SessionController,
  type SessionSummary,
} from '../state';

let lifecycle = 0;
function currentLifecycle(): number {
  return lifecycle;
}
function invalidateLifecycle(): void {
  lifecycle += 1;
}
let currentSelectionTracker: (() => void) | undefined;
function cancelSelectionTracking(): void {
  currentSelectionTracker?.();
}

function cancelSavedPreview(): void {
  void invoke('cancel_saved_transcript').catch(() => undefined);
}

function trackSelection(
  controller: SessionController,
  milestones: ReturnType<typeof startActionMilestones>,
  projectPath: string,
  sessionId: string,
): void {
  currentSelectionTracker?.();
  let stop: WatchStopHandle = () => undefined;
  let readable = false;
  const current = (): boolean =>
    state.activeControllerKey === controller.key &&
    state.activeProjectPath === projectPath &&
    state.activeSessionId === sessionId &&
    !controller.disposed;
  const cancel = (): void => {
    stop();
    milestones.cancel();
    if (currentSelectionTracker === cancel) currentSelectionTracker = undefined;
  };
  currentSelectionTracker = cancel;
  stop = watch(
    () =>
      [
        controller.ready,
        controller.savedContentLoaded,
        controller.messagesLoaded,
        controller.messages.length,
      ] as const,
    () => {
      if (!current()) {
        cancel();
        return;
      }
      if (controller.ready) milestones.mark('ready');
      if (
        !readable &&
        (controller.savedContentLoaded ||
          controller.messagesLoaded ||
          controller.messages.length > 0)
      ) {
        readable = true;
        milestones.mark(
          controller.messagesLoaded
            ? 'readable_rpc'
            : controller.savedContentLoaded
              ? 'readable_saved'
              : 'readable_memory',
        );
      }
      if (controller.messagesLoaded) milestones.mark('hydrated');
    },
    { immediate: true, flush: 'sync' },
  );
  void nextTick(() => {
    if (current()) milestones.afterRender();
    else cancel();
  });
}

function trackSelectionWrite(
  write: Promise<boolean>,
  milestones: ReturnType<typeof startActionMilestones>,
): void {
  void write.then((success) =>
    milestones.mark(success ? 'persisted' : 'persistence_failed'),
  );
}

interface SavedTranscript {
  messages: unknown[] | null;
}

const previewNavigation = new WeakMap<SessionController, number>();
function readSavedSession(
  project: ProjectSummary,
  session: SessionSummary,
  controller: SessionController,
): void {
  if (!session.path || controller.messagesLoaded || controller.messages.length)
    return;
  const navigation = lifecycle;
  const activation = (previewNavigation.get(controller) ?? 0) + 1;
  previewNavigation.set(controller, activation);
  const generation = controller.generation;
  const streamSequence = controller.streamSequence;
  const hydrationSequence = controller.messagesHydrationSequence;
  void invoke<SavedTranscript>('read_saved_transcript', {
    projectPath: project.path,
    sessionId: session.id,
    sessionPath: session.path,
  })
    .then(({ messages }) => {
      if (
        !Array.isArray(messages) ||
        navigation !== lifecycle ||
        previewNavigation.get(controller) !== activation ||
        controller.disposed ||
        (generation !== 0 && controller.generation !== generation) ||
        controller.streamSequence !== streamSequence ||
        controller.messagesHydrationSequence !== hydrationSequence ||
        controller.messagesLoaded ||
        controller.messages.length > 0 ||
        controller.streaming ||
        state.activeControllerKey !== controller.key ||
        state.activeSessionId !== session.id ||
        state.activeProjectPath !== project.path
      )
        return;
      controller.messages = hydrateTranscript(messages, controller.messages);
      controller.savedContentLoaded = true;
    })
    .catch(() => undefined);
}

export {
  currentLifecycle,
  invalidateLifecycle,
  cancelSelectionTracking,
  cancelSavedPreview,
  trackSelection,
  trackSelectionWrite,
  readSavedSession,
};
