/*
 * Stage 5's controller lifecycle classifier and transition helper:
 * `classifyControllerLifecycle`'s priority order over the seven
 * lifecycle-critical booleans, and `setControllerLifecycle`'s before/after
 * comparison, cause, and trace-context linking. `runtime.ts`/`useTau.ts`'s
 * own call sites are covered by their own test files; this file tests the
 * shared mechanism directly.
 */
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SessionController } from './state';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({ accepted: 1, rejected: 0 })),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockClear();
  vi.resetModules();
});

function testController(
  overrides: Partial<SessionController> = {},
): SessionController {
  return {
    key: 'controller-1',
    runtimeId: 'runtime-1',
    projectPath: '/tmp/project',
    sessionId: 'session-1',
    sessionPath: '/tmp/project/session.json',
    sessionName: '',
    phantom: false,
    generation: 0,
    ready: false,
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

describe('classifyControllerLifecycle', () => {
  it('returns idle when no lifecycle flag is set', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(classifyControllerLifecycle(testController())).toBe('idle');
  });

  it('returns ready when only ready is set', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(classifyControllerLifecycle(testController({ ready: true }))).toBe(
      'ready',
    );
  });

  it('treats streaming as working even when working itself is false', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(
      classifyControllerLifecycle(
        testController({ ready: true, streaming: true }),
      ),
    ).toBe('working');
  });

  it('prioritizes syncing over working/ready', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(
      classifyControllerLifecycle(
        testController({ ready: true, working: true, syncing: true }),
      ),
    ).toBe('syncing');
  });

  it('prioritizes stopping over syncing/working/ready', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(
      classifyControllerLifecycle(
        testController({
          ready: true,
          streaming: true,
          syncing: true,
          stopping: true,
        }),
      ),
    ).toBe('stopping');
  });

  it('prioritizes starting over stopping/syncing/working/ready', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(
      classifyControllerLifecycle(
        testController({
          ready: true,
          streaming: true,
          syncing: true,
          stopping: true,
          starting: true,
        }),
      ),
    ).toBe('starting');
  });

  it('prioritizes connecting over everything else', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    expect(
      classifyControllerLifecycle(
        testController({
          ready: true,
          streaming: true,
          syncing: true,
          stopping: true,
          starting: true,
          connectingRemote: true,
        }),
      ),
    ).toBe('connecting');
  });
});

describe('setControllerLifecycle', () => {
  it('applies the patch and records a transition when the derived state changes', async () => {
    const { setControllerLifecycle } = await import('./state');
    const { flushTelemetry } = await import('../lib/telemetry');
    const controller = testController();

    setControllerLifecycle(
      controller,
      { starting: true, ready: false, stopping: false },
      'controller_start',
    );

    expect(controller.starting).toBe(true);
    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([name]) => name === 'ingest_telemetry',
    );
    const records = (
      ingestCall?.[1] as {
        records: Array<{ family: string; attributes: Record<string, unknown> }>;
      }
    ).records;
    const transition = records.find(
      (record) => record.family === 'controller.lifecycle',
    );
    expect(transition?.attributes).toEqual({
      'tau.controller.state.before': 'idle',
      'tau.controller.state.after': 'starting',
      'tau.controller.transition.cause': 'controller_start',
      'tau.session.id': 'session-1',
      'tau.controller.id': 'controller-1',
      'tau.runtime.id': 'runtime-1',
    });
  });

  it('does not record anything when the derived state does not change', async () => {
    const { setControllerLifecycle } = await import('./state');
    const controller = testController({ working: true, streaming: true });

    // Still derives to 'working' (streaming stays true), so no transition
    // is expected even though the raw `working` boolean did change.
    setControllerLifecycle(controller, { working: false }, 'agent_settled');

    expect(controller.working).toBe(false);
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'ingest_telemetry',
      expect.anything(),
    );
  });

  it('carries the given trace context onto the recorded transition', async () => {
    const { setControllerLifecycle } = await import('./state');
    const { flushTelemetry, startActionSpan } =
      await import('../lib/telemetry');
    const action = startActionSpan('message.send');
    const controller = testController();

    setControllerLifecycle(
      controller,
      { working: true },
      'message_send',
      action.context,
    );

    await flushTelemetry();

    const records = mockInvoke.mock.calls
      .filter(([name]) => name === 'ingest_telemetry')
      .flatMap(
        ([, args]) =>
          (
            args as {
              records: Array<{
                family: string;
                traceId?: string;
                spanId?: string;
              }>;
            }
          ).records,
      );
    const transition = records.find(
      (record) => record.family === 'controller.lifecycle',
    );
    expect(transition?.traceId).toBe(action.context?.traceId);
    expect(transition?.spanId).toBe(action.context?.spanId);
  });

  it('never records anything about messages, draft, or status', async () => {
    const { setControllerLifecycle } = await import('./state');
    const { flushTelemetry } = await import('../lib/telemetry');
    const controller = testController({
      draft: 'tau-canary-draft-text',
      status: 'tau-canary-status-text',
    });

    setControllerLifecycle(controller, { working: true }, 'message_send');

    await flushTelemetry();

    const ingestCall = mockInvoke.mock.calls.find(
      ([name]) => name === 'ingest_telemetry',
    );
    expect(JSON.stringify(ingestCall)).not.toContain('tau-canary');
  });
});

describe('buildStateSnapshot', () => {
  it('reports zero counts and an empty draft bucket for an empty workspace', async () => {
    const { buildStateSnapshot, state } = await import('./state');
    state.controllers = [];
    state.extensionDialogs = [];
    state.activeControllerKey = '';

    const snapshot = buildStateSnapshot();

    expect(snapshot).toEqual({
      controllerCount: 0,
      runtimeCount: 0,
      activeControllerCount: 0,
      notificationCount: 0,
      dialogCount: 0,
      transcriptCounts: {
        user: 0,
        assistant: 0,
        tool: 0,
        thinking: 0,
        error: 0,
      },
      draftBucket: 'empty',
      scope: { sessionId: undefined, controllerId: undefined },
    });
  });

  it('counts controllers, runtimes, and transcript entries by kind, never their text', async () => {
    const { buildStateSnapshot, state } = await import('./state');
    const idle = testController({ key: 'idle-1', generation: 0 });
    const running = testController({
      key: 'running-1',
      generation: 2,
      working: true,
      messages: [
        { id: '1', kind: 'user', text: 'tau-canary-user-message' },
        { id: '2', kind: 'assistant', text: 'tau-canary-assistant-message' },
        { id: '3', kind: 'tool', text: 'tau-canary-tool-message' },
        { id: '4', kind: 'thinking', text: 'tau-canary-thinking-message' },
        { id: '5', kind: 'error', text: 'tau-canary-error-message' },
        {
          id: '6',
          kind: 'notice',
          text: 'tau-canary-notification',
          noticeType: 'info',
        },
      ],
    });
    state.controllers = [idle, running];
    state.extensionDialogs = [];
    state.activeControllerKey = 'running-1';

    const snapshot = buildStateSnapshot();

    expect(snapshot.controllerCount).toBe(2);
    expect(snapshot.runtimeCount).toBe(1);
    expect(snapshot.activeControllerCount).toBe(1);
    expect(snapshot.notificationCount).toBe(1);
    expect(snapshot.dialogCount).toBe(0);
    expect(snapshot.transcriptCounts).toEqual({
      user: 1,
      assistant: 1,
      tool: 1,
      thinking: 1,
      error: 1,
    });
    expect(snapshot.scope).toEqual({
      sessionId: 'session-1',
      controllerId: 'running-1',
    });
    expect(JSON.stringify(snapshot)).not.toContain('tau-canary');
  });

  it.each([
    [0, 'empty'],
    [1, 'short'],
    [50, 'short'],
    [51, 'medium'],
    [500, 'medium'],
    [501, 'long'],
  ] as const)(
    'buckets a %i-character draft as %s, never the draft text itself',
    async (length, bucket) => {
      const { buildStateSnapshot, state } = await import('./state');
      const controller = testController({
        draft: 'x'.repeat(length),
      });
      state.controllers = [controller];
      state.activeControllerKey = controller.key;

      const snapshot = buildStateSnapshot();

      expect(snapshot.draftBucket).toBe(bucket);
      if (length > 0) {
        expect(JSON.stringify(snapshot)).not.toContain('x'.repeat(length));
      }
    },
  );
});
