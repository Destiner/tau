import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { reactive } from 'vue';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import { state, type SessionController } from '../../../composables/state';

import { applyPendingSessionSettings, sendPhantomMessage } from './delivery';
import { handleBridgeEvent } from './events';
import {
  canReleaseRuntime,
  startController,
  stopControllerProcess,
} from './lifecycle';
import handleResponse from './responses';
import { invalidateSetup, recoverSetupModel, retrySetup } from './setup';

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
const controllers: SessionController[] = [];
function controller(): SessionController {
  const result = reactive(
    sessionControllerFixture({
      generation: 1,
      ready: true,
      phantom: true,
      draft: '',
      currentModelProvider: 'fallback',
      currentModelId: 'default',
      currentModelName: 'Default',
      currentEffort: 'off',
      pendingPrompt: {
        message: '/skill:synthetic',
        draft: '/skill:synthetic',
        optimisticId: 'optimistic-setup',
        command: false,
        stateRequestId: '',
        messagesRequestId: '',
        selectedModelProvider: 'invented',
        selectedModelId: 'desired',
        selectedModelName: 'Desired',
        selectedEffort: 'high',
        settingsRequestId: '',
        settingsStep: '',
      },
    }),
  );
  controllers.push(result);
  return result;
}
function requests(): Record<string, unknown>[] {
  return mockInvoke.mock.calls.flatMap(([command, args]) =>
    command === 'send_pi'
      ? [(args as { request: Record<string, unknown> }).request]
      : [],
  );
}
async function respond(
  c: SessionController,
  type: string,
  success = true,
  data: unknown = {},
  error = '',
): Promise<void> {
  const request = requests()
    .filter((item) => item.type === type)
    .at(-1);
  expect(request).toBeDefined();
  await handleResponse(c, {
    command: type,
    id: request!.id,
    success,
    data,
    error,
  });
  await Promise.resolve();
}
async function rejectInitial(c: SessionController): Promise<void> {
  await applyPendingSessionSettings(c);
  await respond(
    c,
    'set_model',
    false,
    {},
    'PRIVATE_TOKEN /secret/path authentication expired',
  );
}
async function reconcile(
  c: SessionController,
  model = 'default',
  levels: unknown[] = ['off', 'high'],
  effort = 'off',
): Promise<void> {
  await respond(c, 'get_state', true, {
    model: model
      ? {
          provider:
            model === 'desired' || model === 'alternate'
              ? 'invented'
              : 'fallback',
          id: model,
          name: model,
        }
      : null,
    thinkingLevel: effort,
  });
  await respond(c, 'get_available_thinking_levels', true, { levels });
}

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(undefined);
});
afterEach(() => {
  for (const c of controllers.splice(0)) invalidateSetup(c);
  state.controllers.splice(0);
  state.workspace = null;
  vi.useRealTimers();
});

describe('session-local initial setup recovery', () => {
  it.each([
    ['No API key found for provider', 'authentication'],
    ['No API key for fixture/desired', 'authentication'],
    ['Authentication failed', 'authentication'],
    ['Authentication failure', 'authentication'],
    ['Invalid API key', 'authentication'],
    ['Unauthorized', 'authentication'],
    ['Model provider/desired not found', 'unavailable'],
    ['The selected model is not available', 'unavailable'],
    ['Unsupported model', 'unavailable'],
    ['Credential cache failed to serialize', 'unknown'],
    ['API key metadata is malformed', 'unknown'],
    ['File does not exist', 'unknown'],
    ['Connection timed out', 'unknown'],
    ['', 'unknown'],
  ])(
    'classifies %j with fixed %s copy and no raw details',
    async (raw, kind) => {
      const c = controller();
      await applyPendingSessionSettings(c);
      await respond(
        c,
        'set_model',
        false,
        {},
        `${raw}\nPRIVATE_SETUP_CANARY /private/token`,
      );
      const copy = {
        authentication:
          'Model setup could not authenticate. Check the Pi provider login on the machine running Pi, then retry setup.',
        unavailable:
          'The selected model is unavailable. Choose another model or check the Pi provider configuration, then retry setup.',
        unknown:
          'Model setup failed. Check the model settings and retry setup.',
      };
      expect(c.setupFailure?.message).toBe(copy[kind as keyof typeof copy]);
      expect(JSON.stringify(c)).not.toContain('PRIVATE_SETUP_CANARY');
      expect(JSON.stringify(c)).not.toContain('/private/token');
      expect(c.feedback).toEqual([]);
    },
  );

  it('executes a ready phantom extension command without replaying failed settings', async () => {
    const c = controller();
    await rejectInitial(c);
    await reconcile(c);
    const failure = c.setupFailure;
    // A local picker selection need not match Pi's actual model.
    c.currentModelProvider = 'invented';
    c.currentModelId = 'desired';
    c.currentEffort = 'high';
    state.workspace = {
      activeProjectPath: c.projectPath,
      piPath: '/usr/local/bin/pi',
      projects: [
        {
          path: c.projectPath,
          name: 'project',
          workingDirectory: c.projectPath,
          collapsed: false,
          selected: true,
          sessions: [],
        },
      ],
    };
    const start = requests().length;
    await sendPhantomMessage(c, '/independent', '/independent', true);
    await respond(c, 'get_state', true, {
      model: { provider: 'fallback', id: 'default' },
      thinkingLevel: 'off',
    });
    await respond(c, 'get_available_thinking_levels', true, {
      levels: ['off'],
    });
    await respond(c, 'get_messages', true, { messages: [] });
    expect(
      requests()
        .slice(start)
        .map((r) => r.type),
    ).toEqual([
      'get_state',
      'get_available_thinking_levels',
      'get_messages',
      'prompt',
    ]);
    expect(requests().at(-1)?.message).toBe('/independent');
    expect(c.setupFailure).toBe(failure);
    expect(c.messages).toEqual([]);
    expect(c.feedback).toEqual([]);
  });

  it.each(['state', 'levels'] as const)(
    'does not overwrite newer phantom picker values during the %s read',
    async (phase) => {
      const c = controller();
      await rejectInitial(c);
      const read = requests().at(-1)!;
      if (phase === 'levels') {
        await respond(c, 'get_state', true, {
          model: { provider: 'fallback', id: 'default' },
          thinkingLevel: 'off',
        });
      }
      c.currentModelProvider = 'newer';
      c.currentModelId = 'picker';
      c.currentModelName = 'Picker';
      c.currentEffort = 'low';
      c.efforts = ['off', 'low'];
      if (phase === 'state') {
        await handleResponse(c, {
          command: 'get_state',
          id: read.id,
          success: true,
          data: {
            model: { provider: 'stale', id: 'old' },
            thinkingLevel: 'high',
          },
        });
      } else {
        await respond(c, 'get_available_thinking_levels', true, {
          levels: ['high'],
        });
      }
      expect(c.currentModelProvider).toBe('newer');
      expect(c.currentModelId).toBe('picker');
      expect(c.currentEffort).toBe('low');
      expect(c.efforts).toEqual(['off', 'low']);
      expect(c.setupFailure?.recovering).toBe(false);
    },
  );

  it('does not overwrite an effort-only picker change during read-only reconciliation', async () => {
    const c = controller();
    await rejectInitial(c);
    await respond(c, 'get_state', true, {
      model: { provider: 'fallback', id: 'default' },
      thinkingLevel: 'off',
    });
    c.currentEffort = 'low';
    await respond(c, 'get_available_thinking_levels', true, {
      levels: ['off', 'high'],
    });
    expect(c.currentEffort).toBe('low');
    expect(c.currentModelId).toBe('default');
    expect(c.efforts).toEqual([]);
  });

  it('preserves the draft and reconciles capabilities without history or a global error', async () => {
    const c = controller();
    await rejectInitial(c);
    expect(c.pendingPrompt).toBeUndefined();
    expect(c.draft).toBe('/skill:synthetic');
    expect(c.setupFailure).toMatchObject({
      provider: 'invented',
      modelId: 'desired',
      effort: 'high',
      recovering: false,
    });
    expect(JSON.stringify(c.setupFailure)).not.toContain('PRIVATE_TOKEN');
    expect(c.feedback).toEqual([]);
    await reconcile(c);
    expect(c.currentModelId).toBe('default');
    expect(c.efforts).toEqual(['off', 'high']);
    expect(c.setupFailure).toBeDefined();
    expect(requests().map((r) => r.type)).toEqual([
      'set_model',
      'get_state',
      'get_available_thinking_levels',
    ]);
  });

  it.each([{ levels: [] }, { levels: ['off'] }])(
    'keeps capabilities truthful: %j',
    async ({ levels }) => {
      const c = controller();
      await rejectInitial(c);
      await reconcile(c, 'default', levels);
      expect(c.efforts).toEqual(levels);
    },
  );

  it('does not invent capabilities when no actual model is confirmed', async () => {
    const c = controller();
    await rejectInitial(c);
    await reconcile(c, '', ['off', 'high']);
    expect(c.efforts).toEqual([]);
    expect(c.setupFailure).toBeDefined();
  });

  it('retries preparation and confirms readiness without sending a draft', async () => {
    const c = controller();
    await rejectInitial(c);
    await reconcile(c);
    c.draft = 'edited while recovering';
    const retry = retrySetup(c);
    const count = requests().length;
    await retrySetup(c);
    expect(requests()).toHaveLength(count);
    expect(c.setupFailure?.recovering).toBe(true);
    await respond(c, 'set_model');
    await respond(c, 'set_thinking_level');
    await reconcile(c, 'desired', ['off', 'high'], 'high');
    await retry;
    expect(c.setupFailure).toBeUndefined();
    expect(c.draft).toBe('edited while recovering');
    expect(c.feedback).toEqual([]);
    expect(
      requests().some((r) =>
        ['get_messages', 'prompt'].includes(String(r.type)),
      ),
    ).toBe(false);
  });

  it('keeps unchanged settings on the existing preparation happy path', async () => {
    const c = controller();
    c.currentModelProvider = 'invented';
    c.currentModelId = 'desired';
    c.currentEffort = 'high';
    await applyPendingSessionSettings(c);
    expect(requests().map((request) => request.type)).toEqual([
      'get_available_thinking_levels',
      'get_messages',
    ]);
    expect(c.setupFailure).toBeUndefined();
  });

  it('keeps a failed recovery and failed reconciliation inline', async () => {
    const c = controller();
    await rejectInitial(c);
    await reconcile(c);
    mockInvoke.mockRejectedValueOnce(new Error('PRIVATE_TOKEN'));
    const retry = retrySetup(c);
    await vi.waitFor(() => {
      expect(
        requests().filter((request) => request.type === 'get_state'),
      ).toHaveLength(2);
    });
    await respond(c, 'get_state', false, {}, 'PRIVATE_TOKEN');
    await retry;
    expect(c.setupFailure?.recovering).toBe(false);
    expect(JSON.stringify(c.setupFailure)).not.toContain('PRIVATE_TOKEN');
    expect(c.feedback).toEqual([]);
    expect(c.draft).toBe('/skill:synthetic');
  });

  it('keeps failures and recovery isolated from another session', async () => {
    const c = controller();
    const other = controller();
    other.pendingPrompt = undefined;
    other.draft = 'another draft';
    await rejectInitial(c);
    await reconcile(c);
    expect(other.setupFailure).toBeUndefined();
    expect(other.draft).toBe('another draft');
    expect(other.currentModelId).toBe('default');
    expect(other.feedback).toEqual([]);
  });

  it('keeps the actual applied model when initial effort preparation fails', async () => {
    const c = controller();
    await applyPendingSessionSettings(c);
    await respond(c, 'set_model');
    await respond(c, 'set_thinking_level', false);
    await reconcile(c, 'desired');
    expect(c.currentModelId).toBe('desired');
    expect(c.currentEffort).toBe('off');
    expect(c.setupFailure?.effort).toBe('high');
    expect(c.feedback).toEqual([]);
    expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(1);
    const retry = retrySetup(c);
    expect(requests().at(-1)?.type).toBe('set_thinking_level');
    await respond(c, 'set_thinking_level');
    await reconcile(c, 'desired', ['off', 'high'], 'high');
    await retry;
    expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(1);
    expect(c.setupFailure).toBeUndefined();
    expect(c.draft).toBe('/skill:synthetic');
  });

  it('classifies a repeated effort failure without replaying the confirmed model', async () => {
    const c = controller();
    await applyPendingSessionSettings(c);
    await respond(c, 'set_model');
    await respond(c, 'set_thinking_level', false);
    await reconcile(c, 'desired');
    const retry = retrySetup(c);
    await respond(
      c,
      'set_thinking_level',
      false,
      {},
      'Authentication failed PRIVATE_EFFORT_CANARY /private/effort',
    );
    await reconcile(c, 'desired');
    await retry;
    expect(c.setupFailure?.message).toBe(
      'Model setup could not authenticate. Check the Pi provider login on the machine running Pi, then retry setup.',
    );
    expect(c.setupFailure?.recovering).toBe(false);
    expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(1);
    expect(JSON.stringify(c)).not.toContain('PRIVATE_EFFORT_CANARY');
    expect(JSON.stringify(c)).not.toContain('/private/effort');
    expect(c.feedback).toEqual([]);
  });

  it('does not treat a structured or malformed error as recognized authentication', async () => {
    const c = controller();
    await applyPendingSessionSettings(c);
    await handleResponse(c, {
      command: 'set_model',
      id: requests().at(-1)!.id,
      success: false,
      error: {
        message: 'Unauthorized PRIVATE_STRUCTURED_CANARY /private/auth',
      },
    });
    expect(c.setupFailure?.message).toBe(
      'Model setup failed. Check the model settings and retry setup.',
    );
    expect(JSON.stringify(c)).not.toContain('PRIVATE_STRUCTURED_CANARY');
    expect(c.feedback).toEqual([]);
  });

  it('handles bridge rejection with the same safe inline surface', async () => {
    const c = controller();
    mockInvoke.mockRejectedValueOnce(new Error('PRIVATE_TOKEN /secret/path'));
    await applyPendingSessionSettings(c);
    expect(c.setupFailure).toBeDefined();
    expect(c.draft).toBe('/skill:synthetic');
    expect(c.feedback).toEqual([]);
    await reconcile(c);
  });

  it('keeps bridge error events inline during setup and recovery', async () => {
    const c = controller();
    state.controllers.push(c);
    await applyPendingSessionSettings(c);
    await handleBridgeEvent({
      kind: 'error',
      runtimeId: c.runtimeId,
      generation: c.generation,
    });
    expect(c.setupFailure?.recovering).toBe(false);
    expect(c.feedback).toEqual([]);
    expect(c.draft).toBe('/skill:synthetic');
    const retry = retrySetup(c);
    await handleBridgeEvent({
      kind: 'error',
      runtimeId: c.runtimeId,
      generation: c.generation,
    });
    await retry;
    expect(c.setupFailure?.recovering).toBe(false);
    expect(c.feedback).toEqual([]);
  });

  it('does not swallow an independent command bridge error behind an idle setup failure', async () => {
    const c = controller();
    const command = {
      ...c.pendingPrompt!,
      command: true,
      message: '/independent',
      draft: '/independent',
    };
    state.controllers.push(c);
    await rejectInitial(c);
    await reconcile(c);
    const failure = c.setupFailure;
    c.draft = '';
    c.pendingPrompt = command;
    c.working = true;
    await handleBridgeEvent({
      kind: 'error',
      runtimeId: c.runtimeId,
      generation: c.generation,
    });
    expect(c.pendingPrompt).toBeUndefined();
    expect(c.working).toBe(false);
    expect(c.draft).toBe('/independent');
    expect(c.feedback.length).toBeGreaterThan(0);
    expect(c.setupFailure).toBe(failure);
  });

  it.each(['replacement', 'disposal'] as const)(
    'clears setup failure for %s rather than retaining a recovery gate',
    async (boundary) => {
      const c = controller();
      await rejectInitial(c);
      await reconcile(c);
      if (boundary === 'replacement') invalidateSetup(c);
      else {
        c.disposed = true;
        await stopControllerProcess(c);
      }
      expect(c.setupFailure).toBeUndefined();
    },
  );

  it.each(['generation', 'exit', 'stop', 'restart'] as const)(
    'retains the intended failed selection and gate across %s, cancelling recovery',
    async (boundary) => {
      const c = controller();
      state.controllers.push(c);
      await rejectInitial(c);
      await reconcile(c);
      const failure = c.setupFailure;
      c.setupFailure = undefined;
      expect(canReleaseRuntime(c)).toBe(true);
      c.setupFailure = failure;
      expect(canReleaseRuntime(c)).toBe(false);
      const retry = retrySetup(c);
      const staleMutation = requests().at(-1)!;
      expect(c.setupFailure?.recovering).toBe(true);
      expect(canReleaseRuntime(c)).toBe(false);
      if (boundary === 'stop') await stopControllerProcess(c);
      else if (boundary === 'restart') {
        await startController(c, {
          path: c.projectPath,
          name: 'project',
          workingDirectory: c.projectPath,
          collapsed: false,
          selected: true,
          sessions: [],
        });
      } else
        await handleBridgeEvent({
          runtimeId: c.runtimeId,
          generation: boundary === 'generation' ? 2 : 1,
          kind: boundary === 'generation' ? 'started' : 'exited',
        });
      await retry;
      await handleResponse(c, {
        command: 'set_model',
        id: staleMutation.id,
        success: true,
      });
      expect(c.setupFailure).toMatchObject({
        provider: 'invented',
        modelId: 'desired',
        effort: 'high',
        recovering: false,
      });
      expect(c.currentModelId).toBe('default');
      expect(c.draft).toBe('/skill:synthetic');
      expect(requests().some((request) => request.type === 'prompt')).toBe(
        false,
      );
      if (boundary === 'generation') {
        const next = retrySetup(c);
        expect(requests().at(-1)).toMatchObject({
          type: 'set_model',
          provider: 'invented',
          modelId: 'desired',
        });
        invalidateSetup(c);
        await next;
      }
    },
  );

  it('bounds initial setup and reconciliation timeouts without a global error', async () => {
    vi.useFakeTimers();
    const c = controller();
    await applyPendingSessionSettings(c);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(c.setupFailure).toBeDefined();
    expect(c.pendingPrompt).toBeUndefined();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(c.setupFailure?.recovering).toBe(false);
    expect(c.feedback).toEqual([]);
  });

  it('bounds retries and keeps failures inline even when reconciliation also rejects', async () => {
    vi.useFakeTimers();
    const c = controller();
    await applyPendingSessionSettings(c);
    await handleResponse(c, {
      id: requests()[0]!.id,
      command: 'set_model',
      success: false,
    });
    await vi.advanceTimersByTimeAsync(10_000);
    const retry = retrySetup(c);
    await vi.advanceTimersByTimeAsync(20_000);
    await retry;
    expect(c.setupFailure?.recovering).toBe(false);
    expect(c.feedback).toEqual([]);
    expect(c.draft).toBe('/skill:synthetic');
  });

  it('fences superseded model recovery, including late replies', async () => {
    const c = controller();
    await rejectInitial(c);
    const staleRead = requests().at(-1)!;
    const older = retrySetup(c);
    const staleMutation = requests().at(-1)!;
    const newer = recoverSetupModel(c, 'invented', 'alternate', 'Alternate');
    await handleResponse(c, {
      command: 'get_state',
      id: staleRead.id,
      success: true,
      data: { model: { id: 'stale' } },
    });
    await handleResponse(c, {
      command: 'set_model',
      id: staleMutation.id,
      success: true,
    });
    await vi.waitFor(() =>
      expect(requests().at(-1)?.id).not.toBe(staleMutation.id),
    );
    await respond(c, 'set_model');
    await reconcile(c, 'alternate', ['off'], 'off');
    await Promise.all([older, newer]);
    expect(c.currentModelId).toBe('alternate');
    expect(c.currentEffort).toBe('off');
    expect(c.efforts).toEqual(['off']);
    expect(
      requests().some((request) => request.type === 'set_thinking_level'),
    ).toBe(false);
    expect(c.setupFailure).toBeUndefined();
  });

  it.each(['initial', 'recovery'])(
    'serializes a timed-out %s backend mutation before newer recovery',
    async (phase) => {
      vi.useFakeTimers();
      const c = controller();
      let older: Promise<void> | undefined;
      if (phase === 'recovery') {
        await rejectInitial(c);
        await reconcile(c);
        older = retrySetup(c);
      } else {
        await applyPendingSessionSettings(c);
      }
      const staleWrite = requests().at(-1)!;
      await vi.advanceTimersByTimeAsync(10_000);
      await reconcile(c);
      await older;
      const modelWriteCount = requests().filter(
        (r) => r.type === 'set_model',
      ).length;
      const newer = recoverSetupModel(c, 'invented', 'alternate', 'Alternate');
      await vi.advanceTimersByTimeAsync(0);
      expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(
        modelWriteCount,
      );
      expect(c.setupFailure?.recovering).toBe(true);

      // Pi can apply the old model *after* Tau's waiter has timed out.
      let backendModel = 'desired';
      await handleResponse(c, {
        id: staleWrite.id,
        command: 'set_model',
        success: true,
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(
        modelWriteCount + 1,
      );
      expect(c.setupFailure).toBeDefined();
      expect(backendModel).toBe('desired');
      backendModel = String(requests().at(-1)!.modelId);
      await respond(c, 'set_model');
      await reconcile(c, backendModel, ['off'], 'off');
      await newer;
      expect(backendModel).toBe('alternate');
      expect(c.currentModelId).toBe(backendModel);
      expect(c.setupFailure).toBeUndefined();
      expect(requests().some((r) => r.type === 'prompt')).toBe(false);
    },
  );

  it('keeps unresolved backend writes gated after a bounded recovery wait', async () => {
    vi.useFakeTimers();
    const c = controller();
    await applyPendingSessionSettings(c);
    const staleWrite = requests().at(-1)!;
    await vi.advanceTimersByTimeAsync(10_000);
    await reconcile(c);
    const retry = recoverSetupModel(c, 'invented', 'alternate', 'Alternate');
    await vi.advanceTimersByTimeAsync(10_000);
    await retry;
    expect(c.setupFailure?.recovering).toBe(false);
    expect(requests().filter((r) => r.type === 'set_model')).toHaveLength(1);
    await handleResponse(c, {
      id: staleWrite.id,
      command: 'set_model',
      success: true,
    });
    expect(c.setupFailure).toBeDefined();
    const deliberateRetry = recoverSetupModel(
      c,
      'invented',
      'alternate',
      'Alternate',
    );
    await respond(c, 'set_model');
    await reconcile(c, 'alternate');
    await deliberateRetry;
    expect(c.setupFailure).toBeUndefined();
  });

  it.each([
    { requested: 'max', effective: 'high', levels: ['off', 'high'] },
    { requested: 'high', effective: 'off', levels: ['off'] },
  ] as const)(
    'accepts successful effort clamp from $requested to $effective',
    async ({ requested, effective, levels }) => {
      const c = controller();
      c.pendingPrompt!.selectedEffort = requested;
      await rejectInitial(c);
      await reconcile(c);
      const retry = retrySetup(c);
      await respond(c, 'set_model');
      expect(requests().at(-1)).toMatchObject({
        type: 'set_thinking_level',
        level: requested,
      });
      await respond(c, 'set_thinking_level');
      await reconcile(c, 'desired', [...levels], effective);
      await retry;
      expect(c.currentEffort).toBe(effective);
      expect(c.efforts).toEqual(levels);
      expect(c.setupFailure).toBeUndefined();
      expect(requests().some((r) => r.type === 'prompt')).toBe(false);
    },
  );

  it.each([
    { model: 'default', effort: 'high', levels: ['off', 'high'] },
    { model: 'desired', effort: 'high', levels: ['off'] },
    { model: 'desired', effort: 'off', levels: [] },
  ])(
    'does not accept unconfirmed recovery settings: %j',
    async ({ model, effort, levels }) => {
      const c = controller();
      await rejectInitial(c);
      await reconcile(c);
      const retry = retrySetup(c);
      await respond(c, 'set_model');
      await respond(c, 'set_thinking_level');
      await reconcile(c, model, levels, effort);
      await retry;
      expect(c.setupFailure?.recovering).toBe(false);
      expect(c.setupFailure).toBeDefined();
    },
  );

  it.each(['generation', 'disposed', 'sessionId'] as const)(
    'drops an initial failure after changed %s',
    async (field) => {
      const c = controller();
      await applyPendingSessionSettings(c);
      const request = requests().at(-1)!;
      if (field === 'generation') c.generation += 1;
      else if (field === 'disposed') c.disposed = true;
      else c.sessionId = 'replacement';
      await handleResponse(c, {
        command: 'set_model',
        id: request.id,
        success: false,
      });
      expect(c.setupFailure).toBeUndefined();
      expect(c.feedback).toEqual([]);
    },
  );

  it.each([
    'generation',
    'disposed',
    'sessionId',
    'pendingSettingRequestId',
  ] as const)('fences a changed %s', async (field) => {
    const c = controller();
    await rejectInitial(c);
    await reconcile(c);
    const retry = retrySetup(c);
    const request = requests().at(-1)!;
    if (field === 'generation') c.generation += 1;
    else if (field === 'disposed') c.disposed = true;
    else c[field] = 'newer';
    await handleResponse(c, {
      command: 'set_model',
      id: request.id,
      success: true,
    });
    await retry;
    expect(c.setupFailure).toBeDefined();
    expect(c.currentModelId).toBe('default');
    expect(c.feedback).toEqual([]);
  });
});
