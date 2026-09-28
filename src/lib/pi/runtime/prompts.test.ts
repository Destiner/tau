import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import { state, type SessionController } from '../../../composables/state';

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

describe('prompt delivery', () => {
  it('keeps a phantom first prompt present through identity adoption and empty preflight hydration', async () => {
    const { handleResponse, rpc } = await import('./index');
    const optimisticId = 'optimistic-user-1';
    const phantomId = 'phantom-1';
    const controller = makeController({
      phantom: true,
      sessionId: phantomId,
      sessionPath: '',
      sessionName: '',
      starting: true,
      working: true,
      promptSubmitting: true,
      currentModelProvider: 'fixture',
      currentModelId: 'alpha',
      currentModelName: 'Alpha',
      currentEffort: 'high',
      pendingPrompt: {
        message: 'Keep this visible',
        draft: 'Keep this visible',
        optimisticId,
        command: false,
        stateRequestId: 'state-1',
        messagesRequestId: '',
        selectedModelProvider: 'fixture',
        selectedModelId: 'alpha',
        selectedModelName: 'Alpha',
        selectedEffort: 'high',
        settingsRequestId: '',
        settingsStep: '',
      },
      messages: [
        {
          id: optimisticId,
          kind: 'user',
          text: 'Keep this visible',
          pending: true,
        },
      ],
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = phantomId;
    state.activeControllerKey = controller.key;
    state.controllers.push(controller);
    state.ephemeralSessions.push({
      id: phantomId,
      path: '',
      title: 'New Session',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: true,
      projectPath: controller.projectPath,
      controllerKey: controller.key,
      createdAt: 1,
      phantom: true,
    });

    const messageTransitions: string[][] = [];
    let visibleMessages = controller.messages;
    Object.defineProperty(controller, 'messages', {
      configurable: true,
      get: () => visibleMessages,
      set: (messages: SessionController['messages']) => {
        visibleMessages = messages;
        messageTransitions.push(messages.map((message) => message.id));
      },
    });

    await rpc(controller, { id: 'state-1', type: 'get_state' });
    await handleResponse(controller, {
      id: 'state-1',
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'session-first-prompt',
        sessionFile: '/tmp/project/session-first-prompt.jsonl',
        sessionName: 'First prompt',
        model: { provider: 'fixture', id: 'alpha', name: 'Alpha' },
        thinkingLevel: 'high',
        isStreaming: false,
      },
    });

    expect(controller.sessionId).toBe('session-first-prompt');
    expect(state.ephemeralSessions[0]?.id).toBe('session-first-prompt');
    expect(controller.lastUserMessageAt).toBe(0);
    expect(state.ephemeralSessions[0]?.lastUserMessageAt).toBe(0);
    const messagesRequestId = controller.pendingPrompt?.messagesRequestId;
    expect(messagesRequestId).toBeTruthy();

    await handleResponse(controller, {
      id: messagesRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });

    expect(messageTransitions).toEqual([[optimisticId]]);
    expect(controller.messages).toMatchObject([
      { id: optimisticId, kind: 'user', text: 'Keep this visible' },
    ]);
    expect(controller.pendingPrompt).toBeUndefined();
    expect(controller.submittedPrompt?.optimisticId).toBe(optimisticId);
    expect(controller.lastUserMessageAt).toBeGreaterThan(0);
    expect(state.ephemeralSessions[0]?.lastUserMessageAt).toBe(
      controller.lastUserMessageAt,
    );
  });

  it('does not mark a pending extension command as user activity', async () => {
    const { dispatchPendingPrompt } = await import('./index');
    const controller = makeController({
      pendingPrompt: {
        message: '/mcp',
        draft: '/mcp',
        optimisticId: 'optimistic-command',
        command: true,
        stateRequestId: '',
        messagesRequestId: '',
        selectedModelProvider: '',
        selectedModelId: '',
        selectedModelName: '',
        selectedEffort: 'off',
        settingsRequestId: '',
        settingsStep: '',
      },
    });
    state.ephemeralSessions.push({
      id: controller.sessionId,
      path: controller.sessionPath,
      title: 'Command session',
      lastActive: '',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: true,
      projectPath: controller.projectPath,
      controllerKey: controller.key,
      createdAt: 1,
      phantom: false,
    });

    await dispatchPendingPrompt(controller);

    expect(controller.lastUserMessageAt).toBe(0);
    expect(state.ephemeralSessions[0]?.lastUserMessageAt).toBe(0);
    expect(controller.commandPromptRequestId).toMatch(/^tau-prompt-/);
  });

  it('keeps only a live optimistic prompt through a streaming identity replacement', async () => {
    const { handleResponse, handleRpc } = await import('./index');
    const optimisticId = 'optimistic-user-1';
    const controller = makeController({
      sessionId: 'temporary-session',
      sessionPath: '/tmp/project/temporary-session.jsonl',
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Keep this visible',
        draft: 'Keep this visible',
        accepted: false,
        optimisticId,
      },
      promptSubmitting: true,
      working: true,
      messages: [
        { id: 'old-user', kind: 'user', text: 'Outgoing user row' },
        { id: 'old-assistant', kind: 'assistant', text: 'Outgoing reply' },
        {
          id: optimisticId,
          kind: 'user',
          text: 'Keep this visible',
          pending: true,
        },
        { id: 'old-notice', kind: 'notice', text: 'Outgoing notice' },
      ],
    });

    const messageTransitions: string[][] = [];
    let visibleMessages = controller.messages;
    Object.defineProperty(controller, 'messages', {
      configurable: true,
      get: () => visibleMessages,
      set: (messages: SessionController['messages']) => {
        visibleMessages = messages;
        messageTransitions.push(messages.map((message) => message.id));
      },
    });

    await handleRpc(controller, { type: 'agent_start' });
    expect(
      controller.messages.find((entry) => entry.id === optimisticId),
    ).toMatchObject({ pendingUserEvent: 'optimistic' });

    await handleResponse(controller, {
      id: controller.runStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-session',
        sessionFile: '/tmp/project/replacement-session.jsonl',
        sessionName: 'Replacement',
        isStreaming: true,
      },
    });

    expect(controller.messages).toEqual([
      expect.objectContaining({
        id: optimisticId,
        kind: 'user',
        text: 'Keep this visible',
        pendingUserEvent: 'optimistic',
      }),
    ]);
    expect(messageTransitions.every((ids) => ids.includes(optimisticId))).toBe(
      true,
    );

    const replacementMessagesRequest = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .reverse()
      .find((request) => request.type === 'get_messages');
    await handleResponse(controller, {
      id: String(replacementMessagesRequest?.id),
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'user', content: 'Keep this visible' }],
      },
    });

    expect(controller.messages).toEqual([
      expect.objectContaining({
        id: optimisticId,
        kind: 'user',
        text: 'Keep this visible',
      }),
    ]);
    expect(messageTransitions.every((ids) => ids.includes(optimisticId))).toBe(
      true,
    );
    expect(
      controller.messages.some((entry) => entry.text.startsWith('Outgoing')),
    ).toBe(false);
  });

  it('restores an unconfirmed saved-session prompt when its bridge fails', async () => {
    const { handleBridgeEvent } = await import('./index');
    const controller = makeController({
      promptSubmitting: true,
      working: true,
      draft: 'A newer draft',
      retry: { generation: 1, attempt: 1, reason: 'rate_limit' },
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
    expect(controller.retry).toBeUndefined();
    expect(controller.draft).toBe('  Keep this draft  \n\nA newer draft');
    expect(controller.messages).toEqual([]);
  });

  it('restores a remote phantom prompt once when startup errors then exits', async () => {
    const { handleBridgeEvent } = await import('./index');
    const controller = makeController({
      phantom: true,
      sessionId: '',
      sessionPath: '',
      promptSubmitting: true,
      starting: true,
      working: true,
      connectingRemote: true,
      ready: false,
      draft: 'A newer draft',
      pendingPrompt: {
        message: 'Keep this draft',
        draft: '  Keep this draft  ',
        optimisticId: 'optimistic-1',
        command: false,
        stateRequestId: 'state-1',
        messagesRequestId: '',
        selectedModelProvider: 'fixture',
        selectedModelId: 'alpha',
        selectedModelName: 'Alpha',
        selectedEffort: 'high',
        settingsRequestId: '',
        settingsStep: '',
      },
      messages: [{ id: 'optimistic-1', kind: 'user', text: 'Keep this draft' }],
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: null,
      projects: [
        {
          path: controller.projectPath,
          name: 'Remote project',
          workingDirectory: '/remote/project',
          connectionString: 'ssh fixture@example',
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    state.activeProjectPath = controller.projectPath;
    state.activeControllerKey = controller.key;
    state.controllers.push(controller);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'error',
    });

    expect(controller.pendingPrompt).toBeUndefined();
    expect(controller.promptSubmitting).toBe(false);
    expect(controller.working).toBe(false);
    expect(controller.draft).toBe('  Keep this draft  \n\nA newer draft');
    expect(controller.messages).toEqual([]);
    expect(controller.lastUserMessageAt).toBe(0);
    expect(state.remoteDialogOpen).toBe(true);
    expect(state.remoteRetry?.controllerKey).toBe(controller.key);

    await handleBridgeEvent({
      runtimeId: controller.runtimeId,
      generation: controller.generation,
      kind: 'exited',
      code: 255,
    });

    expect(controller.draft).toBe('  Keep this draft  \n\nA newer draft');
    expect(controller.messages).toEqual([]);
    expect(controller.lastUserMessageAt).toBe(0);
    expect(controller.feedback).toEqual([]);
    expect(state.remoteConnectionError).toBe(
      'The remote Pi process stopped unexpectedly. Check the connection and try again.',
    );
    expect(state.remoteRetry?.controllerKey).toBe(controller.key);
  });

  it('restores an unconfirmed prompt interrupted by a new generation', async () => {
    const { handleBridgeEvent } = await import('./index');
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
    const { handleBridgeEvent } = await import('./index');
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
    const { handleResponse } = await import('./index');
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
    const { sendPhantomMessage } = await import('./index');
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
    expect(feedbackMessage(controller)).toBe(
      'The message could not be sent. Reopen the session and try again.',
    );
  });
});
