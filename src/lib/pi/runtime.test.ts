/*
 * Focused tests for Stage 3's Pi RPC span lifecycle (exact matching,
 * unmatched/duplicate responses, timeout, abandonment, and generation
 * separation) and per-run streaming aggregation. Telemetry primitives
 * (`startRpcSpan`, `recordStreamAggregate`, `invokeTraced`) are mocked so
 * these tests assert on runtime.ts's own bookkeeping, not on the OTel SDK
 * already covered by src/lib/telemetry/index.test.ts.
 */
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { state, type SessionController } from '../../composables/state';
import { FORBIDDEN_CONTENT_CANARIES } from '../telemetry/privacy';

import type { PiBridgeEvent } from './bridge';

const rpcSpans = vi.hoisted(
  () => new Map<string, { end: ReturnType<typeof vi.fn> }>(),
);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock('../telemetry', () => ({
  invokeTraced: vi.fn(async () => undefined),
  recordControllerTransition: vi.fn(),
  recordRpcResponseAnomaly: vi.fn(),
  recordStreamAggregate: vi.fn(),
  startRpcSpan: vi.fn(
    (
      _method: string,
      requestId: string,
      runtimeId: string,
      generation: number,
    ) => {
      const end = vi.fn();
      rpcSpans.set(`${runtimeId}:${generation}:${requestId}`, { end });
      return {
        context: {
          traceId: `trace-${requestId}`,
          spanId: `span-${requestId}`,
          sampled: true,
        },
        end,
      };
    },
  ),
}));

const mockInvoke = vi.mocked(invoke);

function endSpyFor(
  runtimeId: string,
  generation: number,
  requestId: string,
): ReturnType<typeof vi.fn> {
  const span = rpcSpans.get(`${runtimeId}:${generation}:${requestId}`);
  if (!span) throw new Error(`No span registered for ${requestId}`);
  return span.end;
}

function makeController(
  overrides: Partial<SessionController> = {},
): SessionController {
  return {
    key: 'controller-1',
    runtimeId: 'runtime-1',
    projectPath: '/tmp/project',
    sessionId: 'session-1',
    sessionPath: '/tmp/project/session.jsonl',
    sessionName: '',
    phantom: false,
    generation: 1,
    ready: true,
    streaming: false,
    stopping: false,
    starting: false,
    working: false,
    promptSubmitting: false,
    unread: false,
    lastUserMessageAt: 0,
    messages: [],
    messagesLoaded: false,
    historyLayers: [],
    firstVisibleHistoryLayer: 0,
    historyPrefixLength: 0,
    historyRequestId: '',
    localErrors: [],
    draft: '',
    status: '',
    actionError: '',
    currentModelProvider: '',
    currentModelId: '',
    currentModelName: '',
    currentEffort: 'off',
    pendingEffort: '',
    pendingSettingRequestId: '',
    models: [],
    modelScope: [],
    efforts: [],
    commands: [],
    commandsLoaded: false,
    pendingPrompt: undefined,
    bootstrapStateRequestId: '',
    bootstrapSessionPath: '',
    runStateRequestId: '',
    startMessagesRequestId: '',
    commandPromptRequestId: '',
    commandSyncRequestId: '',
    replacementProbeRequestId: '',
    abortProbeRequestId: '',
    connectingRemote: false,
    remoteConnectionTimedOut: false,
    syncing: false,
    lastActiveSequence: 0,
    disposed: false,
    streamSequence: 0,
    ...overrides,
  };
}

beforeEach(async () => {
  mockInvoke.mockClear();
  rpcSpans.clear();
  state.controllers.splice(0);
  state.workspace = null;
  const telemetry = await import('../telemetry');
  vi.mocked(telemetry.recordControllerTransition).mockClear();
  vi.mocked(telemetry.recordRpcResponseAnomaly).mockClear();
  vi.mocked(telemetry.recordStreamAggregate).mockClear();
  vi.mocked(telemetry.invokeTraced).mockClear();
  vi.mocked(telemetry.startRpcSpan).mockClear();
});

describe('prompt delivery', () => {
  it('restores an unconfirmed saved-session prompt when its bridge fails', async () => {
    const { handleBridgeEvent } = await import('./runtime');
    const controller = makeController({
      promptSubmitting: true,
      working: true,
      draft: 'A newer draft',
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Keep this draft',
        draft: '  Keep this draft  ',
        accepted: false,
        optimisticId: 'optimistic-1',
      },
      messages: [{ id: 'optimistic-1', kind: 'user', text: 'Keep this draft' }],
    });
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
    });

    expect(controller.promptSubmitting).toBe(false);
    expect(controller.working).toBe(false);
    expect(controller.submittedPrompt).toBeUndefined();
    expect(controller.draft).toBe('  Keep this draft  \n\nA newer draft');
    expect(controller.messages).toEqual([]);
  });

  it('restores an unconfirmed prompt interrupted by a new generation', async () => {
    const { handleBridgeEvent } = await import('./runtime');
    const controller = makeController({
      promptSubmitting: true,
      working: true,
      draft: 'A newer draft',
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Keep this draft',
        draft: 'Keep this draft',
        accepted: false,
        optimisticId: 'optimistic-1',
      },
      messages: [{ id: 'optimistic-1', kind: 'user', text: 'Keep this draft' }],
    });
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation + 1,
      kind: 'started',
    });

    expect(controller.generation).toBe(2);
    expect(controller.promptSubmitting).toBe(false);
    expect(controller.working).toBe(false);
    expect(controller.submittedPrompt).toBeUndefined();
    expect(controller.draft).toBe('Keep this draft\n\nA newer draft');
    expect(controller.messages).toEqual([]);
  });

  it('does not restore a prompt Pi accepted before its bridge failed', async () => {
    const { handleBridgeEvent } = await import('./runtime');
    const controller = makeController({
      draft: 'A newer draft',
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Keep this draft',
        draft: 'Keep this draft',
        accepted: true,
        optimisticId: 'optimistic-1',
      },
      messages: [{ id: 'optimistic-1', kind: 'user', text: 'Keep this draft' }],
    });
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
    });

    expect(controller.submittedPrompt).toBeUndefined();
    expect(controller.draft).toBe('A newer draft');
    expect(controller.messages).toHaveLength(1);
  });

  it('does not restore an accepted prompt when its response reports failure', async () => {
    const { handleResponse } = await import('./runtime');
    const controller = makeController({
      promptSubmitting: true,
      streaming: true,
      working: true,
      draft: 'A newer draft',
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Keep this draft',
        draft: 'Keep this draft',
        accepted: true,
        optimisticId: 'optimistic-1',
      },
      messages: [{ id: 'optimistic-1', kind: 'user', text: 'Keep this draft' }],
    });

    await handleResponse(controller, {
      id: 'prompt-1',
      command: 'prompt',
      success: false,
    });

    expect(controller.promptSubmitting).toBe(false);
    expect(controller.submittedPrompt).toBeUndefined();
    expect(controller.draft).toBe('A newer draft');
    expect(controller.messages).toHaveLength(1);
    expect(controller.streaming).toBe(true);
    expect(controller.working).toBe(true);
  });

  it('restores a resumed phantom draft when its first request cannot be sent', async () => {
    const { sendPhantomMessage } = await import('./runtime');
    const controller = makeController({
      phantom: true,
      ready: true,
      generation: 1,
      draft: '',
    });
    state.workspace = {
      activeProjectPath: '/tmp/project',
      piPath: '/usr/local/bin/pi',
      projects: [
        {
          path: '/tmp/project',
          name: 'project',
          workingDirectory: '/tmp/project',
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    let rejectSend: ((error: Error) => void) | undefined;
    mockInvoke.mockImplementationOnce(
      () =>
        new Promise<never>((_resolve, reject) => {
          rejectSend = reject;
        }),
    );

    const delivery = sendPhantomMessage(
      controller,
      'Keep this draft',
      '  Keep this draft  ',
      false,
    );
    controller.draft = 'A newer draft';
    rejectSend?.(new Error('Pi is unavailable'));
    await delivery;

    expect(controller.promptSubmitting).toBe(false);
    expect(controller.pendingPrompt).toBeUndefined();
    expect(controller.draft).toBe('  Keep this draft  \n\nA newer draft');
    expect(controller.messages).toEqual([]);
    expect(controller.status).toBe(
      'The message could not be sent. Reopen the session and try again.',
    );
  });
});

describe('skill transcript events', () => {
  it('optimistically keeps a known skill and its prompt together', async () => {
    const { appendOptimisticPrompt } = await import('./runtime');
    const controller = makeController({
      commands: [
        {
          name: 'skill:pr-review',
          source: 'skill',
          description: 'Review a PR',
        },
      ],
    });

    appendOptimisticPrompt(
      controller,
      '/skill:pr-review Focus on correctness',
      'optimistic-1',
    );

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-1',
        kind: 'skill',
        text: '',
        skillName: 'pr-review',
        skillPrompt: 'Focus on correctness',
      },
    ]);
  });

  it('upgrades a raw optimistic command when command metadata arrived late', async () => {
    const { appendOptimisticPrompt, handleRpc } = await import('./runtime');
    const controller = makeController();
    appendOptimisticPrompt(
      controller,
      '/skill:pr-review Focus on correctness',
      'optimistic-user-1',
    );

    await handleRpc(controller, {
      type: 'message_start',
      message: {
        role: 'user',
        content: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
Full review instructions
</skill>

Focus on correctness`,
      },
    });

    expect(controller.messages).toMatchObject([
      {
        id: 'optimistic-user-1',
        kind: 'skill',
        text: 'Full review instructions',
        skillName: 'pr-review',
        skillPrompt: 'Focus on correctness',
      },
    ]);
  });

  it('fills optimistic skill details from the user message event', async () => {
    const { appendOptimisticPrompt, handleRpc } = await import('./runtime');
    const controller = makeController({
      commands: [{ name: 'skill:pr-review', source: 'skill' }],
    });
    appendOptimisticPrompt(controller, '/skill:pr-review', 'optimistic-1');

    await handleRpc(controller, {
      type: 'message_start',
      message: {
        role: 'user',
        content: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
Full review instructions
</skill>`,
      },
    });

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-1',
        kind: 'skill',
        text: 'Full review instructions',
        skillName: 'pr-review',
      },
    ]);
  });
});

describe('compacted history', () => {
  it('loads one earlier layer while keeping the compacted tail in place', async () => {
    const { handleResponse, requestEarlierHistory } = await import('./runtime');
    const controller = makeController({
      messages: [
        {
          id: 'compaction-0',
          kind: 'compaction',
          text: '',
          historyAvailable: true,
          historyLoading: false,
        },
        { id: 'user-1', kind: 'user', text: 'current question' },
      ],
      messagesLoaded: true,
    });

    await requestEarlierHistory(controller);
    expect(controller.historyRequestId).toMatch(/history-/);
    expect(controller.messages[0]).toMatchObject({ historyLoading: true });

    const requestId = controller.historyRequestId;
    await handleResponse(controller, {
      id: requestId,
      command: 'get_entries',
      success: true,
      data: {
        leafId: 'd',
        entries: [
          {
            type: 'message',
            id: 'a',
            parentId: null,
            message: { role: 'user', content: 'root question' },
          },
          {
            type: 'message',
            id: 'b',
            parentId: 'a',
            message: {
              role: 'assistant',
              content: [{ type: 'text', text: 'root reply' }],
            },
          },
          {
            type: 'message',
            id: 'c',
            parentId: 'b',
            message: { role: 'user', content: 'current question' },
          },
          {
            type: 'compaction',
            id: 'd',
            parentId: 'c',
            firstKeptEntryId: 'c',
            summary: 'summary',
          },
        ],
      },
    });

    expect(controller.historyRequestId).toBe('');
    expect(controller.messages.map((message) => message.text)).toEqual([
      'root question',
      'root reply',
      '',
      'current question',
    ]);
    expect(controller.messages[2]).toMatchObject({
      id: 'compaction-0',
      kind: 'compaction',
      historyAvailable: false,
      historyLoading: false,
    });

    await handleResponse(controller, {
      id: 'messages-after-history',
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'compactionSummary', summary: 'summary' },
          { role: 'user', content: 'current question' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'new reply' }],
          },
        ],
      },
    });
    expect(controller.messages.map((message) => message.text)).toEqual([
      'root question',
      'root reply',
      '',
      'current question',
      'new reply',
    ]);
  });

  it('keeps the divider retryable when loading entries fails', async () => {
    const { handleResponse, requestEarlierHistory } = await import('./runtime');
    const controller = makeController({
      messages: [
        {
          id: 'compaction-0',
          kind: 'compaction',
          text: '',
          historyAvailable: true,
          historyLoading: false,
        },
      ],
    });

    await requestEarlierHistory(controller);
    const requestId = controller.historyRequestId;
    await handleResponse(controller, {
      id: requestId,
      command: 'get_entries',
      success: false,
    });

    expect(controller.historyRequestId).toBe('');
    expect(controller.messages[0]).toMatchObject({
      historyAvailable: true,
      historyLoading: false,
    });
    expect(controller.status).toBe(
      'Earlier messages could not be loaded. Try again.',
    );
  });
});

describe('Pi RPC span lifecycle', () => {
  it('never passes RPC request content into telemetry on successful sends', async () => {
    const telemetry = await import('../telemetry');
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    let index = 0;
    for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
      index += 1;
      const id = `req-canary-${index}`;
      await rpc(controller, {
        id,
        type: 'prompt',
        message: canary,
        toolResult: canary,
        extensionValue: canary,
      });
      await handleResponse(controller, {
        id,
        command: 'prompt',
        success: true,
      });
    }

    const telemetryCalls = JSON.stringify(
      vi.mocked(telemetry.startRpcSpan).mock.calls,
    );
    for (const canary of FORBIDDEN_CONTENT_CANARIES.values()) {
      expect(telemetryCalls).not.toContain(canary);
    }
  });

  it('starts a span at request creation and ends it on the exact matching response', async () => {
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    await rpc(controller, { id: 'req-1', type: 'get_state' });
    const telemetry = await import('../telemetry');
    expect(telemetry.startRpcSpan).toHaveBeenCalledWith(
      'get_state',
      'req-1',
      'runtime-1',
      1,
      undefined,
      { sessionId: 'session-1', controllerId: 'controller-1' },
    );
    const end = endSpyFor('runtime-1', 1, 'req-1');
    expect(end).not.toHaveBeenCalled();

    await handleResponse(controller, {
      id: 'req-1',
      command: 'get_state',
      success: true,
    });

    expect(end).toHaveBeenCalledWith('success');
  });

  it('links response-driven state transitions to the matching RPC span', async () => {
    const telemetry = await import('../telemetry');
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController({
      working: true,
      commandPromptRequestId: 'req-transition',
    });

    await rpc(controller, { id: 'req-transition', type: 'prompt' });
    await handleResponse(controller, {
      id: 'req-transition',
      command: 'prompt',
      success: true,
    });

    expect(telemetry.recordControllerTransition).toHaveBeenCalledWith(
      'working',
      'ready',
      'prompt_response',
      expect.objectContaining({ controllerId: 'controller-1' }),
      {
        traceId: 'trace-req-transition',
        spanId: 'span-req-transition',
        sampled: true,
      },
    );
  });

  it('ends with an error outcome for a failed response', async () => {
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    await rpc(controller, { id: 'req-2', type: 'abort' });
    const end = endSpyFor('runtime-1', 1, 'req-2');
    await handleResponse(controller, {
      id: 'req-2',
      command: 'abort',
      success: false,
    });

    expect(end).toHaveBeenCalledWith('error');
  });

  it('leaves other pending spans untouched by an unmatched response', async () => {
    const telemetry = await import('../telemetry');
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    await rpc(controller, { id: 'req-3', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-3');

    await handleResponse(controller, {
      id: 'unrelated-id',
      command: 'get_state',
      success: true,
    });

    expect(end).not.toHaveBeenCalled();
    expect(telemetry.recordRpcResponseAnomaly).toHaveBeenCalledWith(
      'unmatched_or_duplicate',
      'unrelated-id',
      expect.objectContaining({ runtimeId: 'runtime-1' }),
    );
    await handleResponse(controller, {
      id: 'req-3',
      command: 'get_state',
      success: true,
    });
  });

  it('records a duplicate response without double-ending the span', async () => {
    const telemetry = await import('../telemetry');
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    await rpc(controller, { id: 'req-4', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-4');

    await handleResponse(controller, {
      id: 'req-4',
      command: 'get_state',
      success: true,
    });
    expect(end).toHaveBeenCalledTimes(1);

    await handleResponse(controller, {
      id: 'req-4',
      command: 'get_state',
      success: true,
    });
    expect(end).toHaveBeenCalledTimes(1);
    expect(telemetry.recordRpcResponseAnomaly).toHaveBeenCalledWith(
      'unmatched_or_duplicate',
      'req-4',
      expect.objectContaining({ runtimeId: 'runtime-1' }),
    );
  });

  it('abandons an older span when a duplicate request key is registered', async () => {
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController();

    await rpc(controller, { id: 'req-duplicate', type: 'get_state' });
    const first = endSpyFor('runtime-1', 1, 'req-duplicate');
    await rpc(controller, { id: 'req-duplicate', type: 'get_state' });
    const second = endSpyFor('runtime-1', 1, 'req-duplicate');

    expect(first).toHaveBeenCalledWith('abandoned_duplicate_request');
    expect(second).not.toHaveBeenCalled();
    await handleResponse(controller, {
      id: 'req-duplicate',
      command: 'get_state',
      success: true,
    });
    expect(second).toHaveBeenCalledWith('success');
  });

  it('times out a request that never receives a response', async () => {
    vi.useFakeTimers();
    try {
      const { rpc } = await import('./runtime');
      const controller = makeController();

      await rpc(controller, { id: 'req-5', type: 'prompt' });
      const end = endSpyFor('runtime-1', 1, 'req-5');

      await vi.advanceTimersByTimeAsync(10 * 60 * 1000);

      expect(end).toHaveBeenCalledWith('timeout');
    } finally {
      vi.useRealTimers();
    }
  });

  it('abandons pending spans for the runtime and generation on process exit', async () => {
    const { rpc, handleBridgeEvent } = await import('./runtime');
    const controller = makeController();
    state.controllers.push(controller);

    await rpc(controller, { id: 'req-6', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-6');

    await handleBridgeEvent({
      runtimeId: 'runtime-1',
      generation: 1,
      kind: 'exited',
      code: 0,
    } satisfies PiBridgeEvent);

    expect(end).toHaveBeenCalledWith('abandoned_process_exit');
  });

  it('bounds a process failure without exposing raw bridge diagnostics', async () => {
    const {
      handleBridgeEvent,
      piConnectionFailureMessage,
      piProcessExitMessage,
    } = await import('./runtime');
    const controller = makeController({
      starting: true,
      streaming: true,
      working: true,
      stopping: true,
      messages: [
        { id: 'optimistic', kind: 'user', text: 'Keep this prompt' },
        { id: 'partial', kind: 'assistant', text: 'Keep this partial reply' },
      ],
    });
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'stderr',
      message: 'RAW_STDERR_SECRET_SENTINEL',
    });
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
      message: 'RAW_BRIDGE_ERROR_SECRET_SENTINEL',
    });
    expect(controller.status).toBe(piConnectionFailureMessage);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 47,
      message: 'Pi exited with status 47. RAW_EXIT_SECRET_SENTINEL',
    });

    expect(controller).toMatchObject({
      generation: 0,
      ready: false,
      starting: false,
      streaming: false,
      stopping: false,
      working: false,
      status: piProcessExitMessage,
    });
    expect(controller.messages.map(({ text }) => text)).toEqual([
      'Keep this prompt',
      'Keep this partial reply',
    ]);
    expect(JSON.stringify(controller)).not.toContain('RAW_');
  });

  it('abandons pending spans for the old generation on a generation change', async () => {
    const { rpc, handleBridgeEvent } = await import('./runtime');
    const controller = makeController({ generation: 1 });
    state.controllers.push(controller);

    await rpc(controller, { id: 'req-7', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-7');

    await handleBridgeEvent({
      runtimeId: 'runtime-1',
      generation: 2,
      kind: 'started',
    } satisfies PiBridgeEvent);

    expect(end).toHaveBeenCalledWith('abandoned_generation_change');
    expect(controller.generation).toBe(2);
  });

  it('does not abandon a span belonging to a different generation of the same runtime', async () => {
    const { rpc, handleBridgeEvent } = await import('./runtime');
    const controller = makeController({ generation: 1 });
    state.controllers.push(controller);

    await rpc(controller, { id: 'req-8', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-8');

    // A second, unrelated runtime's exit must never touch this one's spans.
    await handleBridgeEvent({
      runtimeId: 'runtime-other',
      generation: 1,
      kind: 'exited',
      code: 0,
    } satisfies PiBridgeEvent);

    expect(end).not.toHaveBeenCalled();
    const { handleResponse } = await import('./runtime');
    await handleResponse(controller, {
      id: 'req-8',
      command: 'get_state',
      success: true,
    });
  });

  it('keeps the runtime state intact when process termination fails', async () => {
    const telemetry = await import('../telemetry');
    const { stopControllerProcess } = await import('./runtime');
    const controller = makeController({
      generation: 1,
      ready: true,
      working: true,
      messages: [{ id: 'message-1', kind: 'assistant', text: 'Keep this' }],
    });
    vi.mocked(telemetry.invokeTraced).mockRejectedValueOnce(
      new Error('Pi did not stop in time. Try again.'),
    );

    await stopControllerProcess(controller);

    expect(controller.generation).toBe(1);
    expect(controller.ready).toBe(true);
    expect(controller.working).toBe(true);
    expect(controller.messages).toHaveLength(1);
    expect(controller.status).toBe(
      'The Pi process could not be closed. Restart Tau and try again.',
    );
  });

  it('abandons pending spans when the controller stops', async () => {
    const { rpc, stopControllerProcess } = await import('./runtime');
    const controller = makeController({ generation: 1 });

    await rpc(controller, { id: 'req-9', type: 'get_state' });
    const end = endSpyFor('runtime-1', 1, 'req-9');

    await stopControllerProcess(controller);

    expect(end).toHaveBeenCalledWith('abandoned_stop');
  });

  it('abandons other pending spans when a response reveals a session replacement', async () => {
    const { rpc, handleResponse } = await import('./runtime');
    const controller = makeController({
      generation: 1,
      sessionId: 'session-1',
      sessionPath: '/tmp/project/session-1.jsonl',
    });

    await rpc(controller, { id: 'req-10', type: 'get_commands' });
    const end = endSpyFor('runtime-1', 1, 'req-10');

    await handleResponse(controller, {
      id: 'req-11',
      command: 'get_state',
      success: true,
      data: {
        model: { provider: 'p', id: 'm', name: 'M' },
        thinkingLevel: 'off',
        sessionId: 'session-2',
        sessionFile: '/tmp/project/session-2.jsonl',
        sessionName: 'Replaced',
        isStreaming: false,
      },
    });

    expect(end).toHaveBeenCalledWith('abandoned_replacement');
  });
});

describe('per-run streaming aggregation', () => {
  it('records one bounded aggregate span per run, never one per delta', async () => {
    const telemetry = await import('../telemetry');
    const { handleRpc } = await import('./runtime');
    const controller = makeController();

    await handleRpc(controller, { type: 'agent_start' });
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'Hello' },
    });
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: ', world' },
    });
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'thinking_delta', delta: 'thinking' },
    });

    expect(telemetry.recordStreamAggregate).not.toHaveBeenCalled();

    await handleRpc(controller, { type: 'agent_settled' });

    expect(telemetry.recordStreamAggregate).toHaveBeenCalledTimes(1);
    expect(telemetry.recordStreamAggregate).toHaveBeenCalledWith(
      'runtime-1',
      1,
      3,
      'Hello'.length + ', world'.length + 'thinking'.length,
      expect.any(Number),
      expect.any(Number),
      { sessionId: 'session-1', controllerId: 'controller-1' },
    );
  });

  it('does not record an aggregate for a run with no deltas', async () => {
    const telemetry = await import('../telemetry');
    const { handleRpc } = await import('./runtime');
    const controller = makeController();

    await handleRpc(controller, { type: 'agent_start' });
    await handleRpc(controller, { type: 'agent_settled' });

    expect(telemetry.recordStreamAggregate).not.toHaveBeenCalled();
  });

  it('keeps aggregates separate per runtime and generation', async () => {
    const telemetry = await import('../telemetry');
    const { handleRpc } = await import('./runtime');
    const first = makeController({ runtimeId: 'runtime-a', generation: 1 });
    const second = makeController({ runtimeId: 'runtime-b', generation: 1 });

    await handleRpc(first, { type: 'agent_start' });
    await handleRpc(second, { type: 'agent_start' });
    await handleRpc(first, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'aaa' },
    });
    await handleRpc(second, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'b' },
    });

    await handleRpc(first, { type: 'agent_settled' });
    expect(telemetry.recordStreamAggregate).toHaveBeenCalledWith(
      'runtime-a',
      1,
      1,
      3,
      expect.any(Number),
      expect.any(Number),
      { sessionId: 'session-1', controllerId: 'controller-1' },
    );

    await handleRpc(second, { type: 'agent_settled' });
    expect(telemetry.recordStreamAggregate).toHaveBeenCalledWith(
      'runtime-b',
      1,
      1,
      1,
      expect.any(Number),
      expect.any(Number),
      { sessionId: 'session-1', controllerId: 'controller-1' },
    );
  });
});
