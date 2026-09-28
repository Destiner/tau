import {
  nextRequestId,
  type SessionController,
} from '../../../composables/state';
import { emptyQueue, type QueueKind, type QueueSubmission } from '../queue';
import { rpcSpanKey, endPendingRpcSpan } from '../rpc-bookkeeping';
import { stringValue } from '../transcript';

import { rpc } from './lifecycle';
import {
  QUEUE_REQUEST_TIMEOUT_MS,
  queueWaiters,
  queuePreparations,
  queueIdentityCurrent,
  recoverQueueDraft,
} from './queue-state';

async function queueRpc(
  controller: SessionController,
  request: Record<string, unknown>,
): Promise<'ok' | 'rejected' | 'uncertain'> {
  const id = stringValue(request.id);
  const method = stringValue(request.type);
  return new Promise((resolve) => {
    let finished = false;
    const complete = (result: 'ok' | 'rejected' | 'uncertain'): void => {
      if (finished) return;
      finished = true;
      const waiter = queueWaiters.get(id);
      if (waiter?.resolve === complete) queueWaiters.delete(id);
      clearTimeout(timer);
      resolve(result);
    };
    const timer = setTimeout(() => {
      endPendingRpcSpan(
        rpcSpanKey(controller.runtimeId, controller.generation, id),
        'timeout',
      );
      complete('uncertain');
    }, QUEUE_REQUEST_TIMEOUT_MS);
    queueWaiters.set(id, {
      controller,
      generation: controller.generation,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
      method,
      resolve: complete,
      timer,
    });
    void rpc(controller, request).catch(() => complete('rejected'));
  });
}

async function prepareQueueModes(
  controller: SessionController,
): Promise<boolean> {
  const existing = queuePreparations.get(controller.key);
  if (existing) return existing;
  const generation = controller.generation;
  const sessionId = controller.sessionId;
  const sessionPath = controller.sessionPath;
  controller.queuePreparing = true;
  const prepare = (async (): Promise<boolean> => {
    for (const [field, method, mode] of [
      ['queueSteeringMode', 'set_steering_mode', 'all'],
      ['queueFollowUpMode', 'set_follow_up_mode', 'one-at-a-time'],
    ] as const) {
      if (controller[field] === mode) continue;
      const result = await queueRpc(controller, {
        id: nextRequestId('queue-mode'),
        type: method,
        mode,
      });
      if (
        !queueIdentityCurrent(controller, generation, sessionId, sessionPath) ||
        result !== 'ok'
      )
        return false;
      controller[field] = mode;
    }
    return true;
  })().finally(() => {
    if (queuePreparations.get(controller.key) !== prepare) return;
    if (queueIdentityCurrent(controller, generation, sessionId, sessionPath))
      controller.queuePreparing = false;
    queuePreparations.delete(controller.key);
  });
  queuePreparations.set(controller.key, prepare);
  return prepare;
}

async function submitQueuedMessage(
  controller: SessionController,
  draft: string,
  kind: QueueKind,
): Promise<void> {
  const text = draft.trim();
  if (
    !text ||
    !controller.ready ||
    !controller.streaming ||
    controller.stopping ||
    controller.compacting ||
    controller.queueClearing ||
    controller.disposed
  )
    return;
  const generation = controller.generation;
  const sessionId = controller.sessionId;
  const sessionPath = controller.sessionPath;
  const id = nextRequestId('queue-prompt');
  const key = kind === 'steer' ? 'steering' : 'followUp';
  const submission: QueueSubmission = {
    id,
    text,
    draft,
    kind,
    version: controller.queueVersion,
    baselineCount: controller.queue[key].filter((item) => item === text).length,
  };
  controller.queueSubmissions.push(submission);
  controller.draft = '';
  controller.queueFeedback = 'Submitting…';
  const prepared = await prepareQueueModes(controller);
  if (!queueIdentityCurrent(controller, generation, sessionId, sessionPath))
    return;
  if (!prepared) {
    controller.queueSubmissions = controller.queueSubmissions.filter(
      (item) => item.id !== submission.id,
    );
    recoverQueueDraft(controller, draft);
    controller.queueFeedback =
      'Could not prepare queue delivery. Check the Pi version and try again.';
    return;
  }
  const result = await queueRpc(controller, {
    id,
    type: 'prompt',
    message: text,
    streamingBehavior: kind,
  });
  if (!queueIdentityCurrent(controller, generation, sessionId, sessionPath))
    return;
  controller.queueSubmissions = controller.queueSubmissions.filter(
    (item) => item.id !== submission.id,
  );
  if (result === 'rejected') {
    recoverQueueDraft(controller, draft);
    controller.queueFeedback = 'Message was not queued. Try again.';
  } else if (result === 'uncertain') {
    controller.queueFailedDrafts.push(draft);
    controller.queueFeedback =
      'Queue confirmation was lost. Check the transcript before resending.';
  } else
    controller.queueFeedback = controller.queueSubmissions.length
      ? 'Submitting…'
      : controller.queueFailedDrafts.length
        ? 'Review unsent messages before retrying.'
        : '';
}

async function clearPendingQueue(controller: SessionController): Promise<void> {
  if (controller.queueClearing || !controller.generation || controller.disposed)
    return;
  if (controller.queuePreparing || controller.queueSubmissions.length) {
    controller.queueFeedback =
      'Wait for queued submissions to finish before clearing.';
    return;
  }
  const generation = controller.generation;
  const sessionId = controller.sessionId;
  const sessionPath = controller.sessionPath;
  controller.queueClearing = true;
  controller.queueFeedback = 'Clearing…';
  const version = controller.queueVersion;
  const result = await queueRpc(controller, {
    id: nextRequestId('clear-queue'),
    type: 'clear_queue',
  });
  if (!queueIdentityCurrent(controller, generation, sessionId, sessionPath))
    return;
  controller.queueClearing = false;
  if (result === 'ok') {
    if (controller.queueVersion === version) {
      controller.queue = emptyQueue();
      controller.queueVersion += 1;
    }
    controller.queueFeedback = '';
  } else
    controller.queueFeedback =
      result === 'uncertain'
        ? 'Clear All was not confirmed. Check the queue before trying again.'
        : 'Could not clear the queue. Upgrade Pi if this command is unavailable, then retry.';
}

export { submitQueuedMessage, clearPendingQueue };
