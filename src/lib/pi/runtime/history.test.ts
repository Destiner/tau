import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
} from '../../../composables/state';
import type { PiBridgeEvent } from '../bridge';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock('../../telemetry', () => ({
  invokeTraced: vi.fn(async () => undefined),
  recordControllerTransition: vi.fn(),
  recordRpcResponseAnomaly: vi.fn(),
  recordStreamAggregate: vi.fn(),
  startRpcSpan: vi.fn(() => ({ context: undefined, end: vi.fn() })),
}));

const mockInvoke = vi.mocked(invoke);

async function dispatchRequest(
  controller: SessionController,
  type: string,
  prefix: string,
): Promise<string> {
  const { rpc } = await import('./index');
  const id = nextRequestId(prefix);
  await rpc(controller, { id, type });
  return id;
}

function feedbackMessage(controller: SessionController): string {
  return (
    [...controller.feedback]
      .reverse()
      .find((incident) => !incident.acknowledged)?.message ?? ''
  );
}

function makeController(
  overrides: Partial<SessionController> = {},
): SessionController {
  return sessionControllerFixture({
    sessionPath: '/tmp/project/session.jsonl',
    generation: 1,
    ready: true,
    ...overrides,
  });
}

beforeEach(async () => {
  mockInvoke.mockClear();
  state.controllers.splice(0);
  state.workspaceFeedback = [];
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.activeControllerKey = '';
  state.workspace = null;
  state.remoteRetry = undefined;
  state.remoteDialogOpen = false;
  state.remoteDialogMode = 'add';
  state.remoteConnectionError = '';
  state.remoteConnecting = false;
  const telemetry = await import('../../telemetry');
  vi.mocked(telemetry.invokeTraced).mockReset();
  vi.mocked(telemetry.invokeTraced).mockResolvedValue(undefined);
});

describe('active compaction', () => {
  it('tracks start and end events per session', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController({ streaming: true, working: true });
    const other = makeController({
      key: 'controller-2',
      runtimeId: 'runtime-2',
      sessionId: 'session-2',
    });

    await handleRpc(controller, { type: 'compaction_start' });
    expect(controller.compacting).toBe(true);
    expect(other.compacting).toBe(false);

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    expect(controller.compacting).toBe(false);
    expect(other.compacting).toBe(false);
  });

  it('clears existing local feedback without removing Pi-owned failures', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController({
      messagesLoaded: true,
      streamSequence: 20,
      localErrors: [
        {
          key: 7,
          label: 'Conversation Not Shortened',
          text: 'anchored failure',
          anchor: 1,
        },
        {
          key: 8,
          label: 'Conversation Not Shortened',
          text: 'unanchored failure',
        },
      ],
      messages: [
        { id: 'user-0', kind: 'user', text: 'retained question' },
        {
          id: 'extension-notify:info',
          kind: 'notice',
          text: 'old info',
          noticeType: 'info',
          anchor: 1,
        },
        {
          id: 'extension-notify:warning',
          kind: 'notice',
          text: 'old warning',
          noticeType: 'warning',
          anchor: 1,
        },
        {
          id: 'extension-notify:error',
          kind: 'notice',
          text: 'old error',
          noticeType: 'error',
          anchor: 1,
        },
        {
          id: 'local-error-7',
          kind: 'error',
          text: 'anchored failure',
          errorLabel: 'Conversation Not Shortened',
          anchor: 1,
        },
        {
          id: 'local-error-8',
          kind: 'error',
          text: 'unanchored failure',
          errorLabel: 'Conversation Not Shortened',
        },
        {
          id: 'assistant-failure',
          kind: 'error',
          text: 'Pi-owned failure',
          errorLabel: 'Reply Failed',
        },
        {
          id: 'tool-failure',
          kind: 'tool',
          text: 'failed command',
          toolCallId: 'tool-1',
          toolName: 'bash',
          toolRunning: false,
          toolErrored: true,
        },
      ],
    });

    await handleRpc(controller, { type: 'compaction_end', result: {} });

    expect(controller.localErrors).toEqual([]);
    expect(controller.messages.map((entry) => entry.id)).toEqual([
      'user-0',
      'assistant-failure',
      'tool-failure',
    ]);
  });

  it('keeps fresh feedback between post-compaction stream rows', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      messagesLoaded: true,
      streaming: true,
      working: true,
      streamSequence: 10,
      messages: [{ id: 'old-user', kind: 'user', text: 'old question' }],
    });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const request = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'first output' },
    });
    await handleRpc(controller, {
      type: 'extension_ui_request',
      id: 'fresh-warning',
      method: 'notify',
      message: 'fresh notice',
      notifyType: 'warning',
    });
    await handleRpc(controller, {
      type: 'tool_execution_start',
      toolCallId: 'later-tool',
      toolName: 'bash',
      args: { command: 'true' },
    });

    await handleResponse(controller, {
      id: request?.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'compactionSummary', summary: 'summary' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'first output' }],
          },
        ],
      },
    });

    expect(
      controller.messages.map((entry) => [entry.kind, entry.text]),
    ).toEqual([
      ['compaction', ''],
      ['assistant', 'first output'],
      ['notice', 'fresh notice'],
      ['tool', 'true'],
    ]);
  });

  it('places fresh feedback observed before initial hydration above later output', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      messagesLoaded: false,
      streaming: true,
      working: true,
    });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const request = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    await handleRpc(controller, {
      type: 'extension_ui_request',
      id: 'fresh-info',
      method: 'notify',
      message: 'before output',
      notifyType: 'info',
    });
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'later output' },
    });

    await handleResponse(controller, {
      id: request?.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'compactionSummary', summary: 'summary' }],
      },
    });

    expect(
      controller.messages.map((entry) => [entry.kind, entry.text]),
    ).toEqual([
      ['compaction', ''],
      ['notice', 'before output'],
      ['assistant', 'later output'],
    ]);
  });

  it('keeps compaction reconciliation pending when the next run starts', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({
      messagesLoaded: true,
      streaming: true,
      working: true,
      streamSequence: 4,
      messages: [{ id: 'old-user', kind: 'user', text: 'old question' }],
    });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const request = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    await handleRpc(controller, {
      type: 'extension_ui_request',
      id: 'between-runs',
      method: 'notify',
      message: 'between runs',
      notifyType: 'info',
    });
    await handleRpc(controller, { type: 'agent_start' });
    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'new run output' },
    });

    expect(controller.compactionReconciliationPending).toBe(true);
    expect(controller.compactionStreamSequence).toBe(4);

    await handleResponse(controller, {
      id: request?.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'compactionSummary', summary: 'summary' }],
      },
    });

    expect(controller.messages.map((entry) => entry.text)).toEqual([
      '',
      'between runs',
      'new run output',
    ]);
  });

  it('immediately reconciles the permanent boundary and keeps later output', async () => {
    const { handleResponse, handleRpc, requestEarlierHistory } =
      await import('./index');
    const controller = makeController({
      streaming: true,
      working: true,
      streamSequence: 5,
      messages: [
        {
          id: 'old-compaction',
          kind: 'compaction',
          text: '',
          historyAvailable: true,
          historyLoading: false,
        },
        { id: 'old-user', kind: 'user', text: 'discarded question' },
        {
          id: 'stream-assistant-4',
          kind: 'assistant',
          text: 'discarded reply',
        },
      ],
    });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const request = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    expect(request?.id).toMatch(/^tau-compaction-messages-/);
    expect(controller.messages[0]).toMatchObject({
      kind: 'compaction',
      historyAvailable: false,
      historyLoading: false,
    });

    await handleRpc(controller, {
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'continued reply' },
    });
    await handleRpc(controller, {
      type: 'tool_execution_start',
      toolCallId: 'continued-tool',
      toolName: 'bash',
      args: { command: 'true' },
    });
    await handleRpc(controller, {
      type: 'tool_execution_end',
      toolCallId: 'continued-tool',
      result: { content: [] },
    });

    await handleResponse(controller, {
      id: request?.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'compactionSummary', summary: 'summary' },
          { role: 'user', content: 'retained question' },
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'continued reply' }],
          },
        ],
      },
    });

    expect(controller.streaming).toBe(true);
    expect(
      controller.messages.map((entry) => [entry.kind, entry.text]),
    ).toEqual([
      ['compaction', ''],
      ['user', 'retained question'],
      ['assistant', 'continued reply'],
      ['tool', 'true'],
    ]);
    expect(controller.messages[0]).toMatchObject({
      historyAvailable: true,
      historyLoading: false,
    });

    await requestEarlierHistory(controller);
    await handleResponse(controller, {
      id: controller.historyRequestId,
      command: 'get_entries',
      success: true,
      data: {
        leafId: 'd',
        entries: [
          {
            type: 'message',
            id: 'a',
            parentId: null,
            message: { role: 'user', content: 'earlier question' },
          },
          {
            type: 'message',
            id: 'b',
            parentId: 'a',
            message: {
              role: 'assistant',
              content: [{ type: 'text', text: 'earlier reply' }],
            },
          },
          {
            type: 'message',
            id: 'c',
            parentId: 'b',
            message: { role: 'user', content: 'retained question' },
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

    expect(
      controller.messages.filter((entry) => entry.text === 'retained question'),
    ).toHaveLength(1);
    expect(controller.messages.map((entry) => entry.text)).toEqual([
      'earlier question',
      'earlier reply',
      '',
      'retained question',
      'continued reply',
      'true',
    ]);
  });

  it('drops an older message hydration after a newer one was dispatched', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({ streaming: true, working: true });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const firstRequest = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    const newerRequestId = await dispatchRequest(
      controller,
      'get_messages',
      'newer-messages',
    );
    await handleResponse(controller, {
      id: newerRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'compactionSummary', summary: 'new' },
          { role: 'user', content: 'new retained tail' },
        ],
      },
    });
    await handleResponse(controller, {
      id: firstRequest?.id,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'user', content: 'stale full transcript' }] },
    });

    expect(controller.messages.map((entry) => entry.text)).toEqual([
      '',
      'new retained tail',
    ]);
  });

  it('resolves overlapping bootstrap and pending-prompt hydration on the newest success', async () => {
    const { handleResponse, rpc } = await import('./index');
    const controller = makeController({
      starting: true,
      syncing: true,
      promptSubmitting: true,
      pendingPrompt: {
        message: 'Pending prompt',
        draft: 'Pending prompt',
        optimisticId: 'optimistic-pending',
        command: false,
        stateRequestId: '',
        messagesRequestId: 'bootstrap-messages',
        selectedModelProvider: '',
        selectedModelId: '',
        selectedModelName: '',
        selectedEffort: 'off',
        settingsRequestId: '',
        settingsStep: '',
      },
    });
    controller.startMessagesRequestId = 'bootstrap-messages';
    await rpc(controller, {
      id: 'bootstrap-messages',
      type: 'get_messages',
    });

    const newestRequestId = await dispatchRequest(
      controller,
      'get_messages',
      'newest-messages',
    );
    expect(controller.startMessagesRequestId).toBe(newestRequestId);

    await handleResponse(controller, {
      id: newestRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'user', content: 'current transcript' }] },
    });
    expect(controller.startMessagesRequestId).toBe('');
    expect(controller.pendingPrompt).toBeUndefined();
    expect(controller.submittedPrompt?.message).toBe('Pending prompt');
    expect(controller.starting).toBe(false);
    expect(controller.syncing).toBe(false);

    await handleResponse(controller, {
      id: 'bootstrap-messages',
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'user', content: 'stale transcript' }] },
    });
    expect(controller.messages.map((entry) => entry.text)).toEqual([
      'current transcript',
      'Pending prompt',
    ]);
  });

  it('transfers every active message hydration purpose before dispatch', async () => {
    const { rpc } = await import('./index');
    const controller = makeController({
      startMessagesRequestId: 'old-bootstrap',
      materializationMessagesRequestId: 'old-materialization',
      pendingPrompt: {
        message: 'Pending prompt',
        draft: 'Pending prompt',
        optimisticId: 'optimistic-pending',
        command: false,
        stateRequestId: '',
        messagesRequestId: 'old-pending',
        selectedModelProvider: '',
        selectedModelId: '',
        selectedModelName: '',
        selectedEffort: 'off',
        settingsRequestId: '',
        settingsStep: '',
      },
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Submitted prompt',
        draft: 'Submitted prompt',
        accepted: true,
        optimisticId: 'optimistic-submitted',
        admissionMessagesRequestId: 'old-admission',
      },
    });

    await rpc(controller, { id: 'newest-messages', type: 'get_messages' });

    expect(controller.startMessagesRequestId).toBe('newest-messages');
    expect(controller.materializationMessagesRequestId).toBe('newest-messages');
    expect(controller.pendingPrompt?.messagesRequestId).toBe('newest-messages');
    expect(controller.submittedPrompt?.admissionMessagesRequestId).toBe(
      'newest-messages',
    );
  });

  it('cleans up and retries the transferred purpose when the newest hydration fails', async () => {
    const {
      handleResponse,
      rpc,
      stopControllerProcess,
      watchingMaterializationVerification,
    } = await import('./index');
    const controller = makeController({
      postSettlementHydration: true,
      starting: true,
      syncing: true,
      startMessagesRequestId: 'older-messages',
      materializationMessagesRequestId: 'older-messages',
    });
    await rpc(controller, { id: 'older-messages', type: 'get_messages' });
    await rpc(controller, { id: 'newest-messages', type: 'get_messages' });

    await handleResponse(controller, {
      id: 'newest-messages',
      command: 'get_messages',
      success: false,
    });

    expect(controller.startMessagesRequestId).toBe('');
    expect(controller.materializationMessagesRequestId).toBe('');
    expect(controller.starting).toBe(false);
    expect(controller.syncing).toBe(false);
    expect(watchingMaterializationVerification(controller)).toBe(true);
    await stopControllerProcess(controller);
  });

  it('cleans up and retries the transferred purpose when the newest hydration send rejects', async () => {
    const { rpc, stopControllerProcess, watchingMaterializationVerification } =
      await import('./index');
    const controller = makeController({
      postSettlementHydration: true,
      starting: true,
      syncing: true,
      startMessagesRequestId: 'older-messages',
      materializationMessagesRequestId: 'older-messages',
    });
    await rpc(controller, { id: 'older-messages', type: 'get_messages' });
    mockInvoke.mockRejectedValueOnce(new Error('transport rejected'));

    await expect(
      rpc(controller, { id: 'newest-messages', type: 'get_messages' }),
    ).rejects.toThrow('transport rejected');

    expect(controller.startMessagesRequestId).toBe('');
    expect(controller.materializationMessagesRequestId).toBe('');
    expect(controller.starting).toBe(false);
    expect(controller.syncing).toBe(false);
    expect(watchingMaterializationVerification(controller)).toBe(true);
    await stopControllerProcess(controller);
  });

  it('keeps a terminal stream error emitted after compaction hydration starts', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const controller = makeController({ streaming: true, working: true });

    await handleRpc(controller, { type: 'compaction_end', result: {} });
    const request = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .find((candidate) => candidate.type === 'get_messages');
    await handleRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        stopReason: 'error',
        errorMessage: '402: out of credits',
      },
    });

    await handleResponse(controller, {
      id: request?.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'compactionSummary', summary: 'summary' }],
      },
    });

    expect(controller.messages).toEqual([
      expect.objectContaining({ kind: 'compaction' }),
      expect.objectContaining({
        id: 'stream-error-0',
        kind: 'error',
        errorLabel: 'Reply Failed',
      }),
    ]);
  });

  it('restores a missed start from get_state and clears it on process exit', async () => {
    const { handleBridgeEvent, handleResponse, rpc } = await import('./index');
    const controller = makeController({ streaming: true, working: true });
    state.controllers.push(controller);
    const compactingRequestId = nextRequestId('command-sync');
    controller.commandSyncRequestId = compactingRequestId;
    await rpc(controller, { id: compactingRequestId, type: 'get_state' });

    await handleResponse(controller, {
      id: compactingRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: true,
        isCompacting: true,
      },
    });
    expect(controller.compacting).toBe(true);

    const finishedRequestId = await dispatchRequest(
      controller,
      'get_state',
      'command-sync',
    );
    await handleResponse(controller, {
      id: finishedRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: true,
        isCompacting: false,
      },
    });
    expect(controller.compacting).toBe(false);

    controller.compacting = true;
    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 0,
    } satisfies PiBridgeEvent);
    expect(controller.compacting).toBe(false);
  });
});

describe('compacted history', () => {
  it('loads one earlier layer while keeping the compacted tail in place', async () => {
    const { handleResponse, requestEarlierHistory } = await import('./index');
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

    const messagesRequestId = await dispatchRequest(
      controller,
      'get_messages',
      'messages',
    );
    await handleResponse(controller, {
      id: messagesRequestId,
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
    const { handleResponse, requestEarlierHistory } = await import('./index');
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
    expect(feedbackMessage(controller)).toBe(
      'Earlier messages could not be loaded. Try again.',
    );
  });
});
