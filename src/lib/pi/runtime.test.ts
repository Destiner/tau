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

import {
  nextRequestId,
  state,
  type SessionController,
} from '../../composables/state';
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

async function dispatchRequest(
  controller: SessionController,
  type: string,
  prefix: string,
): Promise<string> {
  const { rpc } = await import('./runtime');
  const id = nextRequestId(prefix);
  await rpc(controller, { id, type });
  return id;
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
    compacting: false,
    stopping: false,
    starting: false,
    working: false,
    promptSubmitting: false,
    unread: false,
    lastUserMessageAt: 0,
    hasPiTranscript: false,
    materializationVerified: false,
    materializationBarrierRequestId: '',
    postSettlementHydration: false,
    settledAssistantActivity: false,
    materializationStateRequestId: '',
    materializationMessagesRequestId: '',
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
  state.ephemeralSessions.splice(0);
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
  const telemetry = await import('../telemetry');
  vi.mocked(telemetry.recordControllerTransition).mockClear();
  vi.mocked(telemetry.recordRpcResponseAnomaly).mockClear();
  vi.mocked(telemetry.recordStreamAggregate).mockClear();
  vi.mocked(telemetry.invokeTraced).mockClear();
  vi.mocked(telemetry.startRpcSpan).mockClear();
});

describe('project selection persistence', () => {
  function setWorkspaceSessions(
    controller: SessionController,
    registered: boolean,
  ): void {
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
          sessions: registered
            ? [
                {
                  id: controller.sessionId,
                  path: controller.sessionPath,
                  title: 'Session',
                  lastActive: 'now',
                  lastUserMessageAt: 0,
                  sortAt: 1,
                  archived: false,
                  selected: true,
                },
              ]
            : [],
        },
      ],
    };
  }

  it('keeps an unregistered real-ID selection frontend-owned', async () => {
    const telemetry = await import('../telemetry');
    const { persistProjectSelection } = await import('./runtime');
    const controller = makeController({
      sessionId: 'extension-session',
      sessionPath: '/tmp/project/extension-session.jsonl',
      phantom: false,
    });
    setWorkspaceSessions(controller, false);
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(state.workspace!);

    await persistProjectSelection(controller.projectPath, controller);

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'set_active_project',
      { path: controller.projectPath },
      undefined,
    );
    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'set_active_session',
      expect.anything(),
      expect.anything(),
    );
    expect(controller.status).toBe('');
    expect(controller.actionError).toBe('');
    expect(controller.localErrors).toEqual([]);
  });

  it('persists a registered session selection', async () => {
    const telemetry = await import('../telemetry');
    const { persistProjectSelection } = await import('./runtime');
    const controller = makeController();
    setWorkspaceSessions(controller, true);
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(state.workspace!);

    await persistProjectSelection(controller.projectPath, controller);

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'set_active_session',
      {
        projectPath: controller.projectPath,
        sessionId: controller.sessionId,
      },
      undefined,
    );
    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'set_active_project',
      expect.anything(),
      expect.anything(),
    );
  });

  it('warns when a registered session selection cannot be persisted', async () => {
    const telemetry = await import('../telemetry');
    const { persistProjectSelection } = await import('./runtime');
    const controller = makeController();
    setWorkspaceSessions(controller, true);
    vi.mocked(telemetry.invokeTraced).mockRejectedValueOnce(
      new Error('Native persistence failed'),
    );

    await persistProjectSelection(controller.projectPath, controller);

    expect(controller.status).toBe(
      'This selection could not be saved. Select it again.',
    );
  });
});

describe('command-created session durability', () => {
  function addEphemeral(
    controller: SessionController,
    connectionString?: string,
  ): void {
    state.controllers.push(controller);
    state.ephemeralSessions.push({
      id: controller.sessionId,
      path: controller.sessionPath,
      title: controller.sessionName || 'Command session',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: true,
      projectPath: controller.projectPath,
      controllerKey: controller.key,
      createdAt: 1,
      phantom: false,
    });
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          ...(connectionString ? { connectionString } : {}),
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
  }

  function registeredWorkspace(
    controller: SessionController,
    title: string,
    archived = false,
  ): NonNullable<typeof state.workspace> {
    return {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          collapsed: false,
          selected: true,
          sessions: [
            {
              id: controller.sessionId,
              path: controller.sessionPath,
              title,
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived,
              selected: false,
            },
          ],
        },
      ],
    };
  }

  it('does not register an identity reported by command sync alone', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    const commandSyncRequestId = nextRequestId('command-sync');
    controller.commandSyncRequestId = commandSyncRequestId;
    await rpc(controller, { id: commandSyncRequestId, type: 'get_state' });

    await handleResponse(controller, {
      id: commandSyncRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        sessionName: controller.sessionName,
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalled();
    expect(state.ephemeralSessions).toHaveLength(1);
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'keeps a user-only %s replacement ephemeral and discards it on release',
    async (_kind, connectionString) => {
      const telemetry = await import('../telemetry');
      const { handleResponse, rpc, stopControllerProcess } =
        await import('./runtime');
      const controller = makeController({
        sessionId: 'user-only-session',
        sessionPath: '/tmp/project/user-only.jsonl',
        sessionName: 'User only',
        lastUserMessageAt: 42,
      });
      addEphemeral(controller, connectionString);
      const messagesRequestId = nextRequestId('messages');
      controller.materializationMessagesRequestId = messagesRequestId;
      await rpc(controller, {
        id: messagesRequestId,
        type: 'get_messages',
      });

      await handleResponse(controller, {
        id: messagesRequestId,
        command: 'get_messages',
        success: true,
        data: { messages: [{ role: 'user', content: 'Unanswered prompt' }] },
      });

      expect(controller.hasPiTranscript).toBe(true);
      expect(controller.materializationVerified).toBe(false);
      expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
        'register_session',
        expect.anything(),
        expect.anything(),
      );
      expect(state.ephemeralSessions).toHaveLength(1);

      await stopControllerProcess(controller, undefined, false);

      expect(state.ephemeralSessions).toEqual([]);
      expect(state.controllers).toEqual([]);
    },
  );

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'adopts an unverified %s session Pi did write when the process is lost',
    async (_kind, connectionString) => {
      const telemetry = await import('../telemetry');
      const { handleBridgeEvent, handleRpc } = await import('./runtime');
      const controller = makeController({
        sessionId: 'materialized-session',
        sessionPath: '/tmp/project/materialized.jsonl',
        sessionName: 'Materialized',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      // A run this long never settles, so Tau holds no materialization proof.
      // Pi's file exists, so the probe's snapshot lists the session.
      const adopted = registeredWorkspace(controller, 'Materialized');
      vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
        command === 'register_session' ? adopted : state.workspace,
      );

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Partial' },
      });
      await handleBridgeEvent({
        runtimeId: controller.runtimeId,
        generation: controller.generation,
        kind: 'exited',
        code: 1,
      } satisfies PiBridgeEvent);

      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({
          sessionId: 'materialized-session',
          adopted: true,
        }),
        undefined,
      );
      expect(state.workspace).toEqual(adopted);
      expect(state.ephemeralSessions).toEqual([]);
      expect(state.controllers).toHaveLength(1);
      expect(state.controllers[0]?.sessionId).toBe('materialized-session');
    },
  );

  it('keeps a session Pi swapped in while the stop was in flight', async () => {
    const telemetry = await import('../telemetry');
    const { stopControllerProcess } = await import('./runtime');
    const controller = makeController({
      sessionId: 'predecessor-session',
      sessionPath: '/tmp/project/predecessor.jsonl',
      sessionName: 'Predecessor',
    });
    addEphemeral(controller);

    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) => {
      // Pi replaces the session on the live runtime, keeping the generation,
      // while the stop this controller authorised is still awaited.
      if (command === 'stop_pi') {
        controller.sessionId = 'successor-session';
        controller.sessionPath = '/tmp/project/successor.jsonl';
      }
      return undefined as never;
    });

    await stopControllerProcess(controller);

    expect(state.ephemeralSessions).toHaveLength(1);
    expect(state.controllers).toHaveLength(1);
    expect(state.controllers[0]?.sessionId).toBe('successor-session');
    expect(state.controllers[0]?.starting).toBe(true);
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'discards a partial assistant stream Pi never wrote after %s process loss',
    async (_kind, connectionString) => {
      const telemetry = await import('../telemetry');
      const { handleBridgeEvent, handleRpc } = await import('./runtime');
      const controller = makeController({
        sessionId: 'partial-session',
        sessionPath: '/tmp/project/partial.jsonl',
        sessionName: 'Partial stream',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      // Pi wrote no session file, so the snapshot the probe gets back lists
      // nothing and the row stays disposable.
      vi.mocked(telemetry.invokeTraced).mockResolvedValue(state.workspace!);

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Partial' },
      });
      await handleBridgeEvent({
        runtimeId: controller.runtimeId,
        generation: controller.generation,
        kind: 'exited',
        code: 1,
      } satisfies PiBridgeEvent);

      expect(state.ephemeralSessions).toEqual([]);
      expect(state.controllers).toEqual([]);
    },
  );

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'promotes a completed %s assistant after its message_end barrier while the run continues',
    async (_kind, connectionString) => {
      const telemetry = await import('../telemetry');
      const { handleResponse, handleRpc } = await import('./runtime');
      const controller = makeController({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        sessionName: 'Completed work',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      const workspace = registeredWorkspace(controller, 'Completed work');
      let releaseRegistration: (() => void) | undefined;
      const registrationHeld = new Promise<void>((resolve) => {
        releaseRegistration = resolve;
      });
      vi.mocked(telemetry.invokeTraced).mockImplementation(async () => {
        await registrationHeld;
        return workspace;
      });

      await handleRpc(controller, {
        type: 'message_start',
        message: { role: 'assistant', content: [] },
      });
      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: {
          type: 'text_delta',
          delta: 'First finalized reply',
        },
      });
      expect(controller.materializationBarrierRequestId).toBe('');
      expect(telemetry.invokeTraced).not.toHaveBeenCalled();

      await handleRpc(controller, {
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{ type: 'text', text: 'First finalized reply' }],
        },
      });
      const barrierRequestId = controller.materializationBarrierRequestId;
      expect(barrierRequestId).toMatch(/^tau-materialization-barrier-/);

      const promotion = handleResponse(controller, {
        id: barrierRequestId,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          sessionName: controller.sessionName,
          isStreaming: true,
        },
      });
      await vi.waitFor(() => {
        expect(telemetry.invokeTraced).toHaveBeenCalled();
      });
      expect(controller.materializationBarrierRequestId).toBe(barrierRequestId);
      releaseRegistration?.();
      await promotion;

      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({
          sessionId: controller.sessionId,
          adopted: true,
        }),
        undefined,
      );
      expect(controller.materializationVerified).toBe(true);
      expect(controller.streaming).toBe(true);
      expect(controller.working).toBe(true);
      expect(state.ephemeralSessions).toEqual([]);
      expect(
        mockInvoke.mock.calls.some(
          ([, args]) =>
            (args as { request?: { type?: string } })?.request?.type ===
            'get_messages',
        ),
      ).toBe(false);
    },
  );

  it('does not promote when the message_end barrier no longer owns the exact identity', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, handleRpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);

    await handleRpc(controller, {
      type: 'message_end',
      message: { role: 'assistant', content: [] },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;
    controller.sessionId = 'replacement-session';
    controller.sessionPath = '/tmp/project/replacement.jsonl';

    await handleResponse(controller, {
      id: barrierRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'completed-session',
        sessionFile: '/tmp/project/completed.jsonl',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalled();
    expect(controller.materializationVerified).toBe(false);
    expect(state.ephemeralSessions).toHaveLength(1);
  });

  it('registers the completed predecessor when its message_end barrier discovers a replacement', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, handleRpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      sessionName: 'Completed work',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(
      registeredWorkspace(controller, 'Completed work'),
    );

    await handleRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [{ type: 'text', text: 'First finalized reply' }],
      },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;

    await handleResponse(controller, {
      id: barrierRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-session',
        sessionFile: '/tmp/project/replacement.jsonl',
        sessionName: 'Replacement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(
      state.workspace?.projects[0]?.sessions.some(
        (session) => session.id === 'completed-session',
      ),
    ).toBe(true);
    expect(controller.sessionId).toBe('replacement-session');
    expect(controller.materializationVerified).toBe(false);
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'replacement-session' }),
    ]);
  });

  it('keeps a completed predecessor proof when its successor starts before the barrier responds', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, handleRpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'completed-session',
      sessionPath: '/tmp/project/completed.jsonl',
      sessionName: 'Completed work',
      streaming: true,
      working: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(
      registeredWorkspace(controller, 'Completed work'),
    );

    await handleRpc(controller, {
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [{ type: 'text', text: 'First finalized reply' }],
      },
    });
    const barrierRequestId = controller.materializationBarrierRequestId;

    await handleRpc(controller, { type: 'agent_start' });
    expect(controller.materializationBarrierRequestId).toBe(barrierRequestId);
    const successorStateRequestId = controller.runStateRequestId;

    await handleResponse(controller, {
      id: successorStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-session',
        sessionFile: '/tmp/project/replacement.jsonl',
        sessionName: 'Replacement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(controller.sessionId).toBe('replacement-session');
    expect(controller.materializationBarrierRequestId).toBe('');
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'replacement-session' }),
    ]);
  });

  it.each([
    ['local', undefined],
    ['remote', 'ssh://fixture'],
  ])(
    'still promotes a completed %s assistant from settled hydration when its barrier was missed',
    async (_kind, connectionString) => {
      const telemetry = await import('../telemetry');
      const { handleResponse, handleRpc } = await import('./runtime');
      const controller = makeController({
        sessionId: 'completed-session',
        sessionPath: '/tmp/project/completed.jsonl',
        sessionName: 'Completed work',
        streaming: true,
        working: true,
      });
      addEphemeral(controller, connectionString);
      const workspace = {
        activeProjectPath: controller.projectPath,
        piPath: '/usr/bin/pi',
        projects: [
          {
            path: controller.projectPath,
            name: 'Project',
            workingDirectory: controller.projectPath,
            ...(connectionString ? { connectionString } : {}),
            collapsed: false,
            selected: true,
            sessions: [
              {
                id: controller.sessionId,
                path: controller.sessionPath,
                title: controller.sessionName,
                lastActive: 'now',
                lastUserMessageAt: 0,
                sortAt: 1,
                archived: false,
                selected: false,
              },
            ],
          },
        ],
      };
      vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce(workspace);

      await handleRpc(controller, {
        type: 'message_update',
        assistantMessageEvent: { type: 'text_delta', delta: 'Complete reply' },
      });
      expect(telemetry.invokeTraced).not.toHaveBeenCalled();

      await handleRpc(controller, { type: 'agent_settled' });
      const settledState = mockInvoke.mock.calls
        .map(
          ([, args]) =>
            (args as { request?: Record<string, unknown> })?.request,
        )
        .find((request) => request?.type === 'get_state');
      await handleResponse(controller, {
        id: settledState?.id,
        command: 'get_state',
        success: true,
        data: {
          sessionId: controller.sessionId,
          sessionFile: controller.sessionPath,
          sessionName: controller.sessionName,
          isStreaming: false,
        },
      });
      const messagesRequestId = controller.materializationMessagesRequestId;
      expect(messagesRequestId).not.toBe('');

      await handleResponse(controller, {
        id: messagesRequestId,
        command: 'get_messages',
        success: true,
        data: {
          messages: [
            { role: 'user', content: 'Prompt' },
            {
              role: 'assistant',
              content: [{ type: 'text', text: 'Complete reply' }],
            },
          ],
        },
      });

      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({ sessionId: controller.sessionId }),
        undefined,
      );
      expect(controller.materializationVerified).toBe(true);
      expect(state.ephemeralSessions).toEqual([]);
    },
  );

  it('registers a completed predecessor before settlement verification applies its replacement', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      syncing: true,
      messages: [
        { id: 'plan-reply', kind: 'assistant', text: 'Completed plan' },
      ],
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockImplementation(
      async (command, args) => {
        if (command === 'register_session') {
          expect(controller.sessionId).toBe('plan-a');
          expect(state.ephemeralSessions[0]).toMatchObject({
            id: 'plan-a',
            path: '/tmp/project/plan-a.jsonl',
            title: 'Plan',
          });
          expect(args).toMatchObject({
            sessionId: 'plan-a',
            sessionPath: '/tmp/project/plan-a.jsonl',
            sessionName: 'Plan',
            adopted: true,
          });
        }
        return registeredWorkspace;
      },
    );
    const verificationRequestId = nextRequestId('settled-state');
    controller.materializationStateRequestId = verificationRequestId;
    await rpc(controller, { id: verificationRequestId, type: 'get_state' });

    await handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['plan-a']);
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'implement-b',
      path: '/tmp/project/implement-b.jsonl',
      title: 'Implement',
    });
    expect(controller).toMatchObject({
      sessionId: 'implement-b',
      sessionPath: '/tmp/project/implement-b.jsonl',
      sessionName: 'Implement',
      materializationVerified: false,
    });
  });

  it('preserves a settled predecessor when successor start reveals its replacement', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, handleRpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      messages: [{ id: 'plan-user', kind: 'user', text: 'Write the plan' }],
    });
    addEphemeral(controller);
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(registeredWorkspace);

    await handleRpc(controller, { type: 'agent_start' });
    const runStateRequestId = controller.runStateRequestId;
    await handleResponse(controller, {
      id: runStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: true,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({ sessionId: 'plan-a', adopted: true }),
      undefined,
    );
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]?.id).toBe('implement-b');
  });

  it('clears settled predecessor evidence when successor start keeps the identity', async () => {
    const { handleResponse, handleRpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      postSettlementHydration: true,
      settledAssistantActivity: true,
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_start' });
    expect(controller.postSettlementHydration).toBe(true);
    expect(controller.settledAssistantActivity).toBe(true);
    await handleResponse(controller, {
      id: controller.runStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'plan-a',
        sessionFile: '/tmp/project/plan-a.jsonl',
        isStreaming: true,
      },
    });

    expect(controller.postSettlementHydration).toBe(false);
    expect(controller.settledAssistantActivity).toBe(false);
  });

  it('preserves a settled predecessor when a later probe discovers its replacement', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      messages: [{ id: 'plan-user', kind: 'user', text: 'Write the plan' }],
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'plan-a',
              path: '/tmp/project/plan-a.jsonl',
              title: 'Plan',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    };
    vi.mocked(telemetry.invokeTraced).mockResolvedValue(registeredWorkspace);
    const probeRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = probeRequestId;

    await handleResponse(controller, {
      id: probeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).toHaveBeenCalledWith(
      'register_session',
      expect.objectContaining({
        sessionId: 'plan-a',
        sessionPath: '/tmp/project/plan-a.jsonl',
        adopted: true,
      }),
      undefined,
    );
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'plan-a' })]),
    );
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'implement-b',
      path: '/tmp/project/implement-b.jsonl',
      title: 'Implement',
      lastActive: 'now',
    });
    expect(state.ephemeralSessions[0]!.sortAt).toBeGreaterThan(2);
    expect(state.activeSessionId).toBe('implement-b');
    expect(state.activeSessionPath).toBe('/tmp/project/implement-b.jsonl');
  });

  it('keeps a registered predecessor title when a probe discovers its successor', async () => {
    const { handleResponse } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      materializationVerified: true,
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Plan');
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const probeRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = probeRequestId;

    await handleResponse(controller, {
      id: probeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(state.workspace?.projects[0]?.sessions).toEqual([
      expect.objectContaining({ id: 'plan-a', title: 'Plan' }),
    ]);
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({ id: 'implement-b', title: 'Implement' }),
    ]);
  });

  it('keeps the predecessor reachable when registration fails', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse } = await import('./runtime');
    const controller = makeController({
      sessionId: 'plan-a',
      sessionPath: '/tmp/project/plan-a.jsonl',
      sessionName: 'Plan',
      postSettlementHydration: true,
      settledAssistantActivity: true,
      syncing: true,
    });
    addEphemeral(controller);
    vi.mocked(telemetry.invokeTraced).mockRejectedValue(
      new Error('registration failed'),
    );
    const requestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = requestId;

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'implement-b',
        sessionFile: '/tmp/project/implement-b.jsonl',
        sessionName: 'Implement',
        isStreaming: false,
      },
    });

    expect(controller.sessionId).toBe('plan-a');
    expect(state.ephemeralSessions[0]).toMatchObject({
      id: 'plan-a',
      path: '/tmp/project/plan-a.jsonl',
      title: 'Plan',
    });
    expect(controller.status).toBe(
      'This session could not be saved. Continue here, then try reopening it.',
    );
    expect(controller.syncing).toBe(false);
  });

  it('does not register an empty predecessor discovered by settlement verification', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'empty-a',
      sessionPath: '/tmp/project/empty-a.jsonl',
      sessionName: 'Empty',
      postSettlementHydration: true,
    });
    addEphemeral(controller);
    const requestId = nextRequestId('settled-state');
    controller.materializationStateRequestId = requestId;
    await rpc(controller, { id: requestId, type: 'get_state' });

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-b',
        sessionFile: '/tmp/project/replacement-b.jsonl',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'register_session',
      expect.anything(),
      expect.anything(),
    );
  });

  it('does not register a partial predecessor outside settlement verification', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse } = await import('./runtime');
    const controller = makeController({
      sessionId: 'partial-a',
      sessionPath: '/tmp/project/partial-a.jsonl',
      sessionName: 'Partial',
      messages: [{ id: 'partial', kind: 'assistant', text: 'Partial reply' }],
    });
    addEphemeral(controller);
    const requestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = requestId;

    await handleResponse(controller, {
      id: requestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'replacement-b',
        sessionFile: '/tmp/project/replacement-b.jsonl',
        isStreaming: false,
      },
    });

    expect(telemetry.invokeTraced).not.toHaveBeenCalledWith(
      'register_session',
      expect.anything(),
      expect.anything(),
    );
  });

  it('commits a predecessor registration already in flight when its replacement arrives', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
      sessionName: 'Verified A',
      postSettlementHydration: true,
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    let resolveRegistration!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const registration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveRegistration = resolve;
      },
    );
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'register_session' ? await registration : state.workspace,
    );
    const verificationRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = verificationRequestId;
    await rpc(controller, {
      id: verificationRequestId,
      type: 'get_messages',
    });

    const promotion = handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'A is durable' }],
          },
        ],
      },
    });
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'register_session',
        expect.objectContaining({ sessionId: 'verified-a' }),
        undefined,
      );
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    controller.replacementProbeRequestId = replacementRequestId;
    const replacement = handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        sessionName: 'Unverified B',
        isStreaming: true,
      },
    });
    await Promise.resolve();
    expect(controller.sessionId).toBe('verified-a');

    resolveRegistration({
      activeProjectPath: controller.projectPath,
      piPath: '/usr/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          collapsed: false,
          selected: true,
          sessions: [
            {
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 2,
              archived: false,
              selected: true,
            },
          ],
        },
      ],
    });
    await Promise.all([promotion, replacement]);

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['verified-a']);
    expect(state.workspace?.projects[0]?.sessions).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: 'verified-a' })]),
    );
    expect(state.activeSessionId).toBe('unverified-b');
    expect(state.activeSessionPath).toBe('/tmp/project/unverified-b.jsonl');
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });

  it('projects only the latest title from overlapping registrations', async () => {
    const telemetry = await import('../telemetry');
    const { persistSessionName } = await import('./runtime');
    const controller = makeController({
      sessionId: 'title-session',
      sessionPath: '/tmp/project/title-session.jsonl',
      sessionName: 'First title',
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Original title');

    let resolveFirst!: (workspace: NonNullable<typeof state.workspace>) => void;
    let resolveSecond!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const firstRegistration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveFirst = resolve;
      },
    );
    const secondRegistration = new Promise<NonNullable<typeof state.workspace>>(
      (resolve) => {
        resolveSecond = resolve;
      },
    );
    vi.mocked(telemetry.invokeTraced)
      .mockReturnValueOnce(firstRegistration)
      .mockReturnValueOnce(secondRegistration);

    const first = persistSessionName(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1);
    });
    controller.sessionName = 'Second title';
    const second = persistSessionName(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(2);
    });

    resolveSecond(registeredWorkspace(controller, 'Second title'));
    await second;
    expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe(
      'Second title',
    );
    resolveFirst(registeredWorkspace(controller, 'First title'));
    await first;
    expect(state.workspace?.projects[0]?.sessions[0]?.title).toBe(
      'Second title',
    );
    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.map(
          ([, args]) => (args as { sessionName?: string }).sessionName,
        ),
    ).toEqual(['First title', 'Second title']);
  });

  it('does not let a weaker registration overwrite overlapping adoption', async () => {
    const telemetry = await import('../telemetry');
    const { registerConnectedSession } = await import('./runtime');
    const controller = makeController({
      sessionId: 'adoption-session',
      sessionPath: '/tmp/project/adoption-session.jsonl',
      sessionName: 'Session',
    });
    state.controllers.push(controller);
    state.workspace = registeredWorkspace(controller, 'Session', true);

    let resolveOrdinary!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    let resolveAdoption!: (
      workspace: NonNullable<typeof state.workspace>,
    ) => void;
    const ordinaryRegistration = new Promise<
      NonNullable<typeof state.workspace>
    >((resolve) => {
      resolveOrdinary = resolve;
    });
    const adoptionRegistration = new Promise<
      NonNullable<typeof state.workspace>
    >((resolve) => {
      resolveAdoption = resolve;
    });
    vi.mocked(telemetry.invokeTraced)
      .mockReturnValueOnce(ordinaryRegistration)
      .mockReturnValueOnce(adoptionRegistration);

    const ordinary = registerConnectedSession(controller);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(1);
    });
    const adoption = registerConnectedSession(controller, undefined, true);
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledTimes(2);
    });

    resolveAdoption(registeredWorkspace(controller, 'Session'));
    await adoption;
    resolveOrdinary(registeredWorkspace(controller, 'Session', true));
    await ordinary;

    expect(state.workspace?.projects[0]?.sessions[0]?.archived).toBe(false);
    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.map(([, args]) => (args as { adopted?: boolean }).adopted),
    ).toEqual([false, true]);
  });

  it('does not register a replacement that races session selection', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    let resolveSelection!: (workspace: typeof registeredWorkspace) => void;
    const selection = new Promise<typeof registeredWorkspace>((resolve) => {
      resolveSelection = resolve;
    });
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) =>
      command === 'set_active_session' ? await selection : registeredWorkspace,
    );
    const verificationRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = verificationRequestId;
    await rpc(controller, {
      id: verificationRequestId,
      type: 'get_messages',
    });

    const promotion = handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'assistant', content: 'Durable A' }] },
    });
    await vi.waitFor(() => {
      expect(telemetry.invokeTraced).toHaveBeenCalledWith(
        'set_active_session',
        expect.objectContaining({ sessionId: 'verified-a' }),
        undefined,
      );
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        isStreaming: true,
      },
    });
    resolveSelection(registeredWorkspace);
    await promotion;

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => (args as { sessionId: string }).sessionId),
    ).toEqual(['verified-a']);
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });

  it('does not adopt a replacement that races selection fallback', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, rpc } = await import('./runtime');
    const controller = makeController({
      sessionId: 'verified-a',
      sessionPath: '/tmp/project/verified-a.jsonl',
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;
    const registeredWorkspace = {
      ...state.workspace!,
      projects: [
        {
          ...state.workspace!.projects[0]!,
          sessions: [
            {
              id: 'verified-a',
              path: '/tmp/project/verified-a.jsonl',
              title: 'Verified A',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    };
    let resolveAdoption!: (workspace: typeof registeredWorkspace) => void;
    const adoption = new Promise<typeof registeredWorkspace>((resolve) => {
      resolveAdoption = resolve;
    });
    let registrationCount = 0;
    vi.mocked(telemetry.invokeTraced).mockImplementation(async (command) => {
      if (command === 'set_active_session') {
        throw new Error('Missing registry row');
      }
      if (command === 'register_session') {
        registrationCount += 1;
        if (registrationCount === 2) return await adoption;
      }
      return registeredWorkspace;
    });
    const verificationRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = verificationRequestId;
    await rpc(controller, {
      id: verificationRequestId,
      type: 'get_messages',
    });

    const promotion = handleResponse(controller, {
      id: verificationRequestId,
      command: 'get_messages',
      success: true,
      data: { messages: [{ role: 'assistant', content: 'Durable A' }] },
    });
    await vi.waitFor(() => {
      expect(registrationCount).toBe(2);
    });

    const replacementRequestId = await dispatchRequest(
      controller,
      'get_state',
      'replacement-probe',
    );
    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'unverified-b',
        sessionFile: '/tmp/project/unverified-b.jsonl',
        isStreaming: true,
      },
    });
    resolveAdoption(registeredWorkspace);
    await promotion;

    expect(
      vi
        .mocked(telemetry.invokeTraced)
        .mock.calls.filter(([command]) => command === 'register_session')
        .map(([, args]) => ({
          sessionId: (args as { sessionId: string }).sessionId,
          adopted: (args as { adopted?: boolean }).adopted,
        })),
    ).toEqual([
      { sessionId: 'verified-a', adopted: true },
      { sessionId: 'verified-a', adopted: true },
    ]);
    expect(state.ephemeralSessions[0]?.id).toBe('unverified-b');
  });

  it('retries verification after a failed settled message hydration', async () => {
    const telemetry = await import('../telemetry');
    const { handleResponse, probeSessionReplacement, rpc } =
      await import('./runtime');
    const controller = makeController({
      sessionId: 'retry-session',
      sessionPath: '/tmp/project/retry.jsonl',
      postSettlementHydration: true,
      syncing: true,
    });
    addEphemeral(controller);
    const failedMessagesRequestId = nextRequestId('messages');
    controller.materializationMessagesRequestId = failedMessagesRequestId;
    await rpc(controller, {
      id: failedMessagesRequestId,
      type: 'get_messages',
    });

    await handleResponse(controller, {
      id: failedMessagesRequestId,
      command: 'get_messages',
      success: false,
    });

    expect(controller.materializationMessagesRequestId).toBe('');
    expect(controller.postSettlementHydration).toBe(true);
    expect(controller.syncing).toBe(false);

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath,
        isStreaming: false,
      },
    });
    const retryRequestId = controller.materializationMessagesRequestId;
    expect(retryRequestId).not.toBe('');

    vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
      ...state.workspace,
      projects: [
        {
          ...state.workspace!.projects[0],
          sessions: [
            {
              id: controller.sessionId,
              path: controller.sessionPath,
              title: 'Retry session',
              lastActive: 'now',
              lastUserMessageAt: 0,
              sortAt: 1,
              archived: false,
              selected: false,
            },
          ],
        },
      ],
    });
    await handleResponse(controller, {
      id: retryRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          {
            role: 'assistant',
            content: [{ type: 'text', text: 'Retry succeeded' }],
          },
        ],
      },
    });

    expect(controller.materializationVerified).toBe(true);
    expect(controller.postSettlementHydration).toBe(false);
    expect(state.ephemeralSessions).toEqual([]);
  });

  it.each([
    ['effort response', 'get_available_thinking_levels', 'response'],
    ['message response', 'get_messages', 'response'],
    ['effort transport', 'get_available_thinking_levels', 'transport'],
    ['message transport', 'get_messages', 'transport'],
  ] as const)(
    'registers replacement B after a failed %s via the bounded verification timer',
    async (_label, failedMethod, failure) => {
      vi.useFakeTimers();
      try {
        const telemetry = await import('../telemetry');
        const {
          handleResponse,
          handleRpc,
          watchingMaterializationVerification,
          watchingSessionReplacement,
        } = await import('./runtime');
        const controller = makeController({
          sessionId: 'identity-a',
          sessionPath: '/tmp/project/identity-a.jsonl',
        });
        addEphemeral(controller);

        let failedTransport = false;
        if (failure === 'transport') {
          mockInvoke.mockImplementation(async (command, args) => {
            const request = (args as { request?: Record<string, unknown> })
              ?.request;
            if (
              command === 'send_pi' &&
              request?.type === failedMethod &&
              controller.sessionId === 'identity-b' &&
              !failedTransport
            ) {
              failedTransport = true;
              throw new Error('transport unavailable');
            }
            return undefined;
          });
        }

        await handleRpc(controller, { type: 'agent_settled' });
        const discoveringStateRequestId =
          controller.materializationStateRequestId;
        const discovery = handleResponse(controller, {
          id: discoveringStateRequestId,
          command: 'get_state',
          success: true,
          data: {
            sessionId: 'identity-b',
            sessionFile: '/tmp/project/identity-b.jsonl',
            isStreaming: false,
          },
        });
        if (failure === 'transport') await expect(discovery).rejects.toThrow();
        else await discovery;

        expect(watchingSessionReplacement(controller)).toBe(false);
        if (failure === 'response') {
          const failedRequest = mockInvoke.mock.calls
            .filter(([command]) => command === 'send_pi')
            .map(
              ([, args]) =>
                (args as { request: Record<string, unknown> }).request,
            )
            .reverse()
            .find((request) => request.type === failedMethod);
          await handleResponse(controller, {
            id: failedRequest?.id,
            command: failedMethod,
            success: false,
          });
        }
        expect(watchingMaterializationVerification(controller)).toBe(true);

        await vi.advanceTimersByTimeAsync(250);
        const retryStateRequestId = controller.materializationStateRequestId;
        expect(retryStateRequestId).toMatch(/^tau-materialization-retry-/);
        await handleResponse(controller, {
          id: retryStateRequestId,
          command: 'get_state',
          success: true,
          data: {
            sessionId: 'identity-b',
            sessionFile: '/tmp/project/identity-b.jsonl',
            sessionName: 'Replacement B',
            isStreaming: false,
          },
        });
        const retryMessagesRequestId =
          controller.materializationMessagesRequestId;

        vi.mocked(telemetry.invokeTraced).mockResolvedValueOnce({
          ...state.workspace!,
          projects: [
            {
              ...state.workspace!.projects[0],
              sessions: [
                {
                  id: 'identity-b',
                  path: '/tmp/project/identity-b.jsonl',
                  title: 'Replacement B',
                  lastActive: 'now',
                  lastUserMessageAt: 0,
                  sortAt: 1,
                  archived: false,
                  selected: false,
                },
              ],
            },
          ],
        });
        await handleResponse(controller, {
          id: retryMessagesRequestId,
          command: 'get_messages',
          success: true,
          data: {
            messages: [
              {
                role: 'assistant',
                content: [{ type: 'text', text: 'B completed' }],
              },
            ],
          },
        });

        expect(controller.materializationVerified).toBe(true);
        expect(watchingMaterializationVerification(controller)).toBe(false);
        expect(state.ephemeralSessions).toEqual([]);
        expect(
          vi
            .mocked(telemetry.invokeTraced)
            .mock.calls.some(
              ([command, args]) =>
                command === 'register_session' &&
                (args as { sessionId: string }).sessionId === 'identity-b',
            ),
        ).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    },
  );

  it('stops retrying and releases an unverified identity after the bounded policy', async () => {
    vi.useFakeTimers();
    try {
      const { handleResponse, rpc, watchingMaterializationVerification } =
        await import('./runtime');
      const controller = makeController({
        postSettlementHydration: true,
        syncing: true,
      });
      addEphemeral(controller);
      const failedStateRequestId = nextRequestId('settled-state');
      controller.materializationStateRequestId = failedStateRequestId;
      await rpc(controller, { id: failedStateRequestId, type: 'get_state' });

      mockInvoke.mockImplementation(async (command, args) => {
        const request = (args as { request?: Record<string, unknown> })
          ?.request;
        if (
          command === 'send_pi' &&
          request?.type === 'get_state' &&
          String(request.id).startsWith('tau-materialization-retry-')
        ) {
          throw new Error('transport unavailable');
        }
        return undefined;
      });
      await handleResponse(controller, {
        id: failedStateRequestId,
        command: 'get_state',
        success: false,
      });

      expect(watchingMaterializationVerification(controller)).toBe(true);
      await vi.advanceTimersByTimeAsync(250);
      await vi.advanceTimersByTimeAsync(750);
      await vi.advanceTimersByTimeAsync(1_500);
      await vi.runAllTimersAsync();

      expect(watchingMaterializationVerification(controller)).toBe(false);
      expect(controller.postSettlementHydration).toBe(false);
      expect(controller.generation).toBe(0);
      expect(state.ephemeralSessions).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels a pending materialization retry when the controller is released', async () => {
    vi.useFakeTimers();
    try {
      const {
        handleResponse,
        rpc,
        stopControllerProcess,
        watchingMaterializationVerification,
      } = await import('./runtime');
      const controller = makeController({
        postSettlementHydration: true,
        syncing: true,
      });
      addEphemeral(controller);
      const failedRequestId = nextRequestId('settled-state');
      controller.materializationStateRequestId = failedRequestId;
      await rpc(controller, { id: failedRequestId, type: 'get_state' });
      await handleResponse(controller, {
        id: failedRequestId,
        command: 'get_state',
        success: false,
      });
      expect(watchingMaterializationVerification(controller)).toBe(true);

      const sendsBeforeRelease = mockInvoke.mock.calls.filter(
        ([command]) => command === 'send_pi',
      ).length;
      await stopControllerProcess(controller, undefined, false);
      await vi.runAllTimersAsync();

      expect(watchingMaterializationVerification(controller)).toBe(false);
      expect(
        mockInvoke.mock.calls.filter(([command]) => command === 'send_pi'),
      ).toHaveLength(sendsBeforeRelease);
    } finally {
      vi.useRealTimers();
    }
  });

  it('ignores a predecessor state failure while verifying its replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./runtime');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    const predecessorRequestId = controller.materializationStateRequestId;
    expect(predecessorRequestId).toMatch(/^tau-settled-state-/);

    await probeSessionReplacement(controller, false);
    const replacementRequestId = controller.replacementProbeRequestId;
    expect(replacementRequestId).toMatch(/^tau-replacement-probe-/);

    await handleResponse(controller, {
      id: replacementRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    expect(verificationRequestId).toMatch(/^tau-messages-/);
    expect(controller.generation).toBe(1);
    expect(controller.syncing).toBe(true);

    await handleResponse(controller, {
      id: predecessorRequestId,
      command: 'get_state',
      success: false,
    });

    expect(controller.disposed).toBe(false);
    expect(controller.generation).toBe(1);
    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.syncing).toBe(true);
    expect(controller.materializationStateRequestId).toBe('');
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
    expect(state.ephemeralSessions).toEqual([
      expect.objectContaining({
        id: 'identity-b',
        path: '/tmp/project/identity-b.jsonl',
      }),
    ]);
  });

  it('ignores a predecessor state success while hydrating its replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./runtime');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    const predecessorRequestId = controller.materializationStateRequestId;
    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    controller.messages = [
      { id: 'identity-b-message', kind: 'assistant', text: 'B transcript' },
    ];

    await handleResponse(controller, {
      id: predecessorRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        sessionName: 'Identity A',
        isStreaming: false,
        thinkingLevel: 'off',
      },
    });

    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('ignores predecessor message and effort successes after replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./runtime');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    await handleResponse(controller, {
      id: controller.materializationStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        isStreaming: false,
      },
    });
    const predecessorMessagesRequestId =
      controller.materializationMessagesRequestId;
    const outboundRequests = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      );
    const predecessorEffortRequestId = String(
      [...outboundRequests]
        .reverse()
        .find((request) => request.type === 'get_available_thinking_levels')
        ?.id,
    );

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    const replacementEffortRequest = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      )
      .reverse()
      .find((request) => request.type === 'get_available_thinking_levels');
    await handleResponse(controller, {
      id: replacementEffortRequest?.id,
      command: 'get_available_thinking_levels',
      success: true,
      data: { levels: ['high', 'max'] },
    });
    controller.messages = [
      { id: 'identity-b-message', kind: 'assistant', text: 'B transcript' },
    ];

    await handleResponse(controller, {
      id: predecessorMessagesRequestId,
      command: 'get_messages',
      success: true,
      data: {
        messages: [{ role: 'assistant', content: 'Stale A transcript' }],
      },
    });

    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );

    await handleResponse(controller, {
      id: predecessorEffortRequestId,
      command: 'get_available_thinking_levels',
      success: true,
      data: { levels: ['off'] },
    });

    expect(controller.sessionId).toBe('identity-b');
    expect(controller.sessionPath).toBe('/tmp/project/identity-b.jsonl');
    expect(controller.messages).toEqual([
      expect.objectContaining({ id: 'identity-b-message' }),
    ]);
    expect(controller.efforts).toEqual(['high', 'max']);
    expect(controller.syncing).toBe(true);
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('ignores predecessor message and effort failures after replacement', async () => {
    const { handleResponse, handleRpc, probeSessionReplacement } =
      await import('./runtime');
    const controller = makeController({
      sessionId: 'identity-a',
      sessionPath: '/tmp/project/identity-a.jsonl',
    });
    addEphemeral(controller);

    await handleRpc(controller, { type: 'agent_settled' });
    await handleResponse(controller, {
      id: controller.materializationStateRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-a',
        sessionFile: '/tmp/project/identity-a.jsonl',
        isStreaming: false,
      },
    });
    const predecessorMessagesRequestId =
      controller.materializationMessagesRequestId;
    const sentRequests = mockInvoke.mock.calls
      .filter(([command]) => command === 'send_pi')
      .map(
        ([, input]) => (input as { request: Record<string, unknown> }).request,
      );
    const predecessorEffortRequestId = String(
      [...sentRequests]
        .reverse()
        .find((request) => request.type === 'get_available_thinking_levels')
        ?.id,
    );
    expect(predecessorMessagesRequestId).toMatch(/^tau-messages-/);
    expect(predecessorEffortRequestId).toMatch(/^tau-efforts-/);

    await probeSessionReplacement(controller, false);
    await handleResponse(controller, {
      id: controller.replacementProbeRequestId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: 'identity-b',
        sessionFile: '/tmp/project/identity-b.jsonl',
        isStreaming: false,
      },
    });
    const verificationRequestId = controller.materializationMessagesRequestId;
    expect(verificationRequestId).toMatch(/^tau-messages-/);
    expect(verificationRequestId).not.toBe(predecessorMessagesRequestId);

    await handleResponse(controller, {
      id: predecessorMessagesRequestId,
      command: 'get_messages',
      success: false,
    });
    await handleResponse(controller, {
      id: predecessorEffortRequestId,
      command: 'get_available_thinking_levels',
      success: false,
    });

    expect(controller.disposed).toBe(false);
    expect(controller.generation).toBe(1);
    expect(controller.sessionId).toBe('identity-b');
    expect(controller.syncing).toBe(true);
    expect(controller.status).toBe('');
    expect(controller.materializationMessagesRequestId).toBe(
      verificationRequestId,
    );
  });

  it('removes a command-only session when its runtime is released', async () => {
    const { stopControllerProcess } = await import('./runtime');
    const controller = makeController({
      generation: 1,
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
    });
    addEphemeral(controller);

    await stopControllerProcess(controller, undefined, false);

    expect(state.ephemeralSessions).toEqual([]);
    expect(state.controllers).toEqual([]);
  });

  it('removes a command-only notification session when the user leaves', async () => {
    const { removeEmptyActivePhantom } = await import('./runtime');
    const controller = makeController({
      sessionId: 'command-session',
      sessionPath: '/tmp/project/command-session.jsonl',
      sessionName: 'Usage',
      messages: [{ id: 'notice', kind: 'notice', text: 'Usage: 10%' }],
    });
    addEphemeral(controller);
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
    state.activeControllerKey = controller.key;

    removeEmptyActivePhantom();

    expect(state.ephemeralSessions).toEqual([]);
    expect(state.controllers).toEqual([]);
    expect(state.activeSessionId).toBe('');
  });
});

describe('prompt delivery', () => {
  it('keeps a phantom first prompt present through identity adoption and empty preflight hydration', async () => {
    const { handleResponse, rpc } = await import('./runtime');
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
  });

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

  it('restores a remote phantom prompt once when startup errors then exits', async () => {
    const { handleBridgeEvent } = await import('./runtime');
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
    expect(controller.status).toBe(
      'The remote Pi process stopped unexpectedly. Check the connection and try again.',
    );
    expect(state.remoteConnectionError).toBe(controller.status);
    expect(state.remoteRetry?.controllerKey).toBe(controller.key);
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

describe('extension failures', () => {
  it('uses reviewed status copy for extension_error without retaining or recording raw details', async () => {
    const telemetry = await import('../telemetry');
    const { handleRpc } = await import('./runtime');
    const controller = makeController();
    const rawDetails = Array.from(FORBIDDEN_CONTENT_CANARIES).join(' ');

    await handleRpc(controller, {
      type: 'extension_error',
      extensionPath: rawDetails,
      event: rawDetails,
      error: rawDetails,
    });

    expect(controller.status).toBe(
      'A Pi extension failed. Review the extension setup and try again.',
    );
    expect(controller.messages).toEqual([]);
    expect(JSON.stringify(controller)).not.toContain(rawDetails);
    expect(telemetry.invokeTraced).not.toHaveBeenCalled();
    expect(telemetry.startRpcSpan).not.toHaveBeenCalled();
    expect(telemetry.recordRpcResponseAnomaly).not.toHaveBeenCalled();
    expect(telemetry.recordStreamAggregate).not.toHaveBeenCalled();
  });
});

describe('ordinary user transcript events', () => {
  it('reconciles transformed Pi input into the composer optimistic row', async () => {
    const { handleRpc } = await import('./runtime');
    const controller = makeController({
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Original input',
        draft: 'Original input',
        accepted: true,
        optimisticId: 'optimistic-user-1',
      },
      messages: [
        {
          id: 'optimistic-user-1',
          kind: 'user',
          text: 'Original input',
          pending: true,
        },
        {
          id: 'extension-notify:1',
          kind: 'notice',
          text: 'Transforming input',
          anchor: 1,
        },
        {
          id: 'local-error-1',
          kind: 'error',
          text: 'A local warning',
          anchor: 1,
        },
      ],
    });

    await handleRpc(controller, { type: 'agent_start' });
    await handleRpc(controller, {
      type: 'message_start',
      message: { role: 'user', content: 'Transformed input' },
    });

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-user-1',
        kind: 'user',
        text: 'Transformed input',
      },
      {
        id: 'extension-notify:1',
        kind: 'notice',
        text: 'Transforming input',
        anchor: 1,
      },
      {
        id: 'local-error-1',
        kind: 'error',
        text: 'A local warning',
        anchor: 1,
      },
    ]);
  });

  it('appends every repeated extension or steering event and ignores empty content', async () => {
    const { handleRpc } = await import('./runtime');
    const controller = makeController();
    const repeated = {
      type: 'message_start',
      message: { role: 'user', content: 'Injected by extension' },
    };

    await handleRpc(controller, repeated);
    await handleRpc(controller, repeated);
    await handleRpc(controller, {
      type: 'message_start',
      message: { role: 'user', content: [{ type: 'image', data: 'hidden' }] },
    });

    expect(controller.messages).toEqual([
      {
        id: 'stream-user-0',
        kind: 'user',
        text: 'Injected by extension',
      },
      {
        id: 'stream-user-1',
        kind: 'user',
        text: 'Injected by extension',
      },
    ]);
  });

  it('isolates user events by runtime and generation', async () => {
    const { handleBridgeEvent } = await import('./runtime');
    const selected = makeController();
    const other = makeController({
      key: 'controller-2',
      runtimeId: 'runtime-2',
      sessionId: 'session-2',
    });
    state.controllers.push(selected, other);
    const line = JSON.stringify({
      type: 'message_start',
      message: { role: 'user', content: 'Only the matching runtime' },
    });

    await handleBridgeEvent({
      runtimeId: other.runtimeId,
      generation: other.generation,
      kind: 'rpc',
      line,
    });
    await handleBridgeEvent({
      runtimeId: selected.runtimeId,
      generation: selected.generation - 1,
      kind: 'rpc',
      line,
    });

    expect(selected.messages).toEqual([]);
    expect(other.messages).toMatchObject([
      { kind: 'user', text: 'Only the matching runtime' },
    ]);
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
        pending: true,
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

describe('active compaction', () => {
  it('tracks start and end events per session', async () => {
    const { handleRpc } = await import('./runtime');
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

  it('restores a missed start from get_state and clears it on process exit', async () => {
    const { handleBridgeEvent, handleResponse, rpc } =
      await import('./runtime');
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

  it('ends extension UI response spans after the write without tracking a Pi response', async () => {
    const { pendingRpcCount, rpc, stopControllerProcess } =
      await import('./runtime');
    const controller = makeController();
    const pendingBefore = pendingRpcCount();

    await rpc(controller, {
      id: 'extension-response-1',
      type: 'extension_ui_response',
      value: 'Approved',
    });

    const end = endSpyFor('runtime-1', 1, 'extension-response-1');
    expect(end).toHaveBeenCalledTimes(1);
    expect(end).toHaveBeenCalledWith('success');
    expect(pendingRpcCount()).toBe(pendingBefore);

    await stopControllerProcess(controller);

    expect(end).toHaveBeenCalledTimes(1);
  });

  it('records a failed extension UI response write without leaving it pending', async () => {
    const { pendingRpcCount, rpc } = await import('./runtime');
    const controller = makeController();
    const pendingBefore = pendingRpcCount();
    mockInvoke.mockRejectedValueOnce(new Error('transport rejected'));

    await expect(
      rpc(controller, {
        id: 'extension-response-2',
        type: 'extension_ui_response',
        cancelled: true,
      }),
    ).rejects.toThrow('transport rejected');

    const end = endSpyFor('runtime-1', 1, 'extension-response-2');
    expect(end).toHaveBeenCalledTimes(1);
    expect(end).toHaveBeenCalledWith('error');
    expect(pendingRpcCount()).toBe(pendingBefore);
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

  it('does not clear newer bookkeeping when an old transport send rejects', async () => {
    const { rpc } = await import('./runtime');
    const controller = makeController({
      postSettlementHydration: true,
      materializationMessagesRequestId: 'old-messages',
      syncing: true,
    });
    let rejectSend!: (error: Error) => void;
    mockInvoke.mockImplementationOnce(
      () =>
        new Promise<never>((_resolve, reject) => {
          rejectSend = reject;
        }),
    );

    const send = rpc(controller, {
      id: 'old-messages',
      type: 'get_messages',
    });
    controller.generation = 2;
    controller.sessionId = 'replacement';
    controller.sessionPath = '/tmp/project/replacement.jsonl';
    controller.materializationMessagesRequestId = 'new-messages';
    rejectSend(new Error('transport rejected'));

    await expect(send).rejects.toThrow('transport rejected');
    expect(controller.materializationMessagesRequestId).toBe('new-messages');
    expect(controller.syncing).toBe(true);
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
    await rpc(controller, { id: 'req-11', type: 'get_state' });

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
