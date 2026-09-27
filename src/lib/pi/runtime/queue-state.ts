import {
  stashInterruptedQueueDrafts,
  type SessionController,
} from '../../../composables/state';
import { emptyQueue, queueHasWork } from '../queue';
import { rpcSpanKey, endPendingRpcSpan } from '../rpc-bookkeeping';

const QUEUE_REQUEST_TIMEOUT_MS = 8_000;

interface QueueWaiter {
  controller: SessionController;
  generation: number;
  sessionId: string;
  sessionPath: string;
  method: string;
  resolve: (result: 'ok' | 'rejected' | 'uncertain') => void;
  timer: ReturnType<typeof setTimeout>;
}

const queueWaiters = new Map<string, QueueWaiter>();

const queuePreparations = new Map<string, Promise<boolean>>();

function queueIdentityCurrent(
  controller: SessionController,
  generation: number,
  sessionId: string,
  sessionPath: string,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === generation &&
    controller.sessionId === sessionId &&
    controller.sessionPath === sessionPath
  );
}

function resetQueue(
  controller: SessionController,
  interrupted = false,
  replacement = false,
): void {
  const hadWork = queueHasWork(controller.queue, controller.queueSubmissions);
  if (replacement) {
    stashInterruptedQueueDrafts(
      controller,
      [
        ...controller.queueFailedDrafts,
        ...controller.queueSubmissions.map((submission) => submission.draft),
      ],
      controller.queue.steering.length > 0 ||
        controller.queue.followUp.length > 0,
    );
    controller.queueFailedDrafts = [];
  }
  for (const [id, waiter] of queueWaiters) {
    if (
      waiter.controller.key !== controller.key ||
      waiter.controller.runtimeId !== controller.runtimeId
    )
      continue;
    queueWaiters.delete(id);
    endPendingRpcSpan(
      rpcSpanKey(controller.runtimeId, waiter.generation, id),
      'abandoned_replacement',
    );
    waiter.resolve('uncertain');
  }
  queuePreparations.delete(controller.key);
  if (!replacement) {
    for (const submission of controller.queueSubmissions)
      recoverQueueDraft(controller, submission.draft);
  }
  controller.queue = emptyQueue();
  controller.queueVersion += 1;
  controller.queueSubmissions = [];
  controller.queueSteeringMode = '';
  controller.queueFollowUpMode = '';
  controller.queuePreparing = false;
  controller.queueClearing = false;
  if (replacement) controller.queueFeedback = '';
  else if (interrupted && hadWork)
    controller.queueFeedback =
      'Pending messages were lost when Pi disconnected. They were not resent.';
}

function recoverQueueDraft(controller: SessionController, draft: string): void {
  if (!controller.draft) controller.draft = draft;
  else controller.queueFailedDrafts.push(draft);
}

export {
  QUEUE_REQUEST_TIMEOUT_MS,
  queueWaiters,
  queuePreparations,
  queueIdentityCurrent,
  resetQueue,
  recoverQueueDraft,
};
