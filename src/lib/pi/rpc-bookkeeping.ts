import type { SessionController } from '../../composables/state';
import { recordStreamAggregate } from '../telemetry';
import type { PiRpcOutcome } from '../telemetry/attributes';
import type { TraceContext } from '../telemetry/trace-context';

// Each request owns its timeout and span; runtime/generation keys fence concurrent sessions.
const RPC_SPAN_TIMEOUT_MS = 10 * 60 * 1000;

interface PendingRpcSpan {
  end: (outcome: PiRpcOutcome) => void;
  context?: TraceContext;
  dispatchSnapshot: RpcDispatchSnapshot;
  method: string;
  timeoutHandle: ReturnType<typeof setTimeout>;
  startedAt: number;
}

const pendingRpcSpans = new Map<string, PendingRpcSpan>();
// Command execution may outlive the telemetry span. Keep only its dispatch
// correlation until Pi replies or the owning runtime is released.
const commandDispatches = new Map<string, number>();

function retainCommandDispatch(key: string, generation: number): void {
  commandDispatches.set(key, generation);
}

function takeCommandDispatch(key: string): number | undefined {
  const generation = commandDispatches.get(key);
  commandDispatches.delete(key);
  return generation;
}

function discardCommandDispatch(key: string): void {
  commandDispatches.delete(key);
}

function rpcSpanKey(
  runtimeId: string,
  generation: number,
  requestId: string,
): string {
  return `${runtimeId}:${generation}:${requestId}`;
}

function registerPendingRpcSpan(
  key: string,
  end: (outcome: PiRpcOutcome) => void,
  dispatchSnapshot: RpcDispatchSnapshot,
  method: string,
  context?: TraceContext,
): void {
  endPendingRpcSpan(key, 'abandoned_duplicate_request');
  const timeoutHandle = setTimeout(() => {
    endPendingRpcSpan(key, 'timeout');
  }, RPC_SPAN_TIMEOUT_MS);
  pendingRpcSpans.set(key, {
    end,
    context,
    dispatchSnapshot,
    method,
    timeoutHandle,
    startedAt: Date.now(),
  });
}

function pendingRpcCount(): number {
  return pendingRpcSpans.size;
}

function oldestPendingRpcAgeMs(): number {
  const now = Date.now();
  let oldest = 0;
  for (const pending of pendingRpcSpans.values()) {
    const age = now - pending.startedAt;
    if (age > oldest) oldest = age;
  }
  return oldest;
}

interface EndPendingRpcResult {
  matched: boolean;
  context?: TraceContext;
  dispatchSnapshot?: RpcDispatchSnapshot;
  method?: string;
}

function endPendingRpcSpan(
  key: string,
  outcome: PiRpcOutcome,
): EndPendingRpcResult {
  const pending = pendingRpcSpans.get(key);
  if (!pending) return { matched: false };
  pendingRpcSpans.delete(key);
  clearTimeout(pending.timeoutHandle);
  pending.end(outcome);
  return {
    matched: true,
    context: pending.context,
    dispatchSnapshot: pending.dispatchSnapshot,
    method: pending.method,
  };
}

function abandonPendingRpcSpans(
  runtimeId: string,
  generation: number,
  outcome: PiRpcOutcome,
  retainedRequestId?: string,
): void {
  const prefix = `${runtimeId}:${generation}:`;
  for (const key of pendingRpcSpans.keys()) {
    if (key.startsWith(prefix) && key !== `${prefix}${retainedRequestId}`)
      endPendingRpcSpan(key, outcome);
  }
  for (const key of commandDispatches.keys()) {
    if (key.startsWith(prefix) && key !== `${prefix}${retainedRequestId}`)
      commandDispatches.delete(key);
  }
}

interface StreamAggregate {
  deltaCount: number;
  characterCount: number;
  startedAt: number;
  sessionId: string;
  controllerId: string;
}

const streamAggregates = new Map<string, StreamAggregate>();

function streamAggregateKey(runtimeId: string, generation: number): string {
  return `${runtimeId}:${generation}`;
}

function resetStreamAggregate(runtimeId: string, generation: number): void {
  streamAggregates.delete(streamAggregateKey(runtimeId, generation));
}

function recordStreamDelta(controller: SessionController, delta: string): void {
  if (!delta) return;
  const key = streamAggregateKey(controller.runtimeId, controller.generation);
  const aggregate =
    streamAggregates.get(key) ??
    ({
      deltaCount: 0,
      characterCount: 0,
      startedAt: Date.now(),
      sessionId: controller.sessionId,
      controllerId: controller.key,
    } satisfies StreamAggregate);
  aggregate.deltaCount += 1;
  aggregate.characterCount += delta.length;
  streamAggregates.set(key, aggregate);
}

function flushStreamAggregate(runtimeId: string, generation: number): void {
  const key = streamAggregateKey(runtimeId, generation);
  const aggregate = streamAggregates.get(key);
  if (!aggregate) return;
  streamAggregates.delete(key);
  if (aggregate.deltaCount === 0) return;
  recordStreamAggregate(
    runtimeId,
    generation,
    aggregate.deltaCount,
    aggregate.characterCount,
    aggregate.startedAt,
    Date.now(),
    {
      sessionId: aggregate.sessionId,
      controllerId: aggregate.controllerId,
    },
  );
}

interface RpcDispatchSnapshot {
  generation: number;
  sessionId: string;
  sessionPath: string;
  messagesHydrationSequence: number;
  sessionNameRevision: number;
  materializationBarrierRequestId: string;
  materializationStateRequestId: string;
  materializationMessagesRequestId: string;
  pendingPrompt: SessionController['pendingPrompt'];
  submittedPrompt: SessionController['submittedPrompt'];
}

export {
  rpcSpanKey,
  registerPendingRpcSpan,
  pendingRpcCount,
  oldestPendingRpcAgeMs,
  endPendingRpcSpan,
  abandonPendingRpcSpans,
  resetStreamAggregate,
  recordStreamDelta,
  flushStreamAggregate,
  retainCommandDispatch,
  takeCommandDispatch,
  discardCommandDispatch,
  type RpcDispatchSnapshot,
};
