import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
} from '../../../composables/state';
import { TAU_RELOAD_COMMAND } from '../extension-command';

import { clearReload, startReload } from './reload';

import { handleResponse, handleRpc, rpc } from './index';

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

const helper = {
  name: TAU_RELOAD_COMMAND,
  source: 'extension' as const,
  description: 'private',
};
function controller(
  overrides: Partial<SessionController> = {},
): SessionController {
  const c = sessionControllerFixture({
    generation: 1,
    ready: true,
    messagesLoaded: true,
    commandsLoaded: true,
    commands: [helper],
    ...overrides,
  });
  state.controllers.push(c);
  return c;
}
async function acknowledge(c: SessionController): Promise<void> {
  await handleResponse(c, {
    id: c.reload!.requestId,
    command: 'prompt',
    success: true,
    data: { disposition: 'handled' },
  });
}
async function answer(
  c: SessionController,
  method: string,
  data: unknown,
  success = true,
): Promise<void> {
  const id = Object.keys(c.reload!.reads).find(
    (id) => c.reload!.reads[id] === method,
  )!;
  expect(id).toBeTruthy();
  await handleResponse(c, { id, command: method, success, data });
}
async function finish(c: SessionController): Promise<void> {
  await answer(c, 'get_state', {
    sessionId: c.sessionId,
    sessionFile: c.sessionPath,
    isStreaming: false,
  });
  await answer(c, 'get_commands', {
    commands: [helper, { name: 'fresh', source: 'extension' }],
  });
  await answer(c, 'get_available_models', {
    models: [{ provider: 'test', id: 'new' }],
  });
  await answer(c, 'get_available_thinking_levels', { levels: ['off', 'high'] });
  expect(c.reload).toBeDefined();
  await answer(c, 'get_messages', { messages: [] });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(invoke).mockClear();
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.workspace = null;
  state.activeControllerKey = '';
});
afterEach(() => {
  for (const c of state.controllers) clearReload(c);
  vi.useRealTimers();
});

describe('session-owned Pi reload', () => {
  it('dispatches the verified helper once, without a transcript or model preparation, and holds all refreshes', async () => {
    const c = controller();
    await startReload(c, ' /reload ');
    const operation = c.reload;
    await startReload(c, '/reload');
    expect(c.reload).toBe(operation);
    expect(c.messages).toEqual([]);
    expect(c.submittedPrompt).toBeUndefined();
    const requests = vi
      .mocked(invoke)
      .mock.calls.filter(([name]) => name === 'send_pi')
      .map(([, args]) => args);
    expect(JSON.stringify(requests)).toContain(`/${TAU_RELOAD_COMMAND}`);
    expect(JSON.stringify(requests)).not.toContain('set_model');
    await acknowledge(c);
    await finish(c);
    expect(c.reload).toBeUndefined();
    expect(c.commands.some((command) => command.name === 'fresh')).toBe(true);
    expect(c.models[0]?.id).toBe('new');
    expect(c.efforts).toEqual(['off', 'high']);
    await startReload(c, '/reload');
    expect(c.reload?.requestId).not.toBe(operation?.requestId);
  });

  it.each([
    { commands: [] },
    { commands: [helper, helper] },
    { commands: [{ ...helper, source: 'prompt' }] },
  ])(
    'fails closed for unavailable or ambiguous discovery %j',
    async ({ commands }) => {
      const c = controller({
        commands: commands as SessionController['commands'],
      });
      c.draft = 'new text';
      await startReload(c, ' /reload ');
      expect(c.reload).toBeUndefined();
      expect(c.draft).toContain('new text');
      expect(c.draft).toContain('/reload');
      expect(
        vi.mocked(invoke).mock.calls.filter(([name]) => name === 'send_pi'),
      ).toHaveLength(0);
    },
  );

  it('waits for exact discovery after reordered cold-session messages and bypasses failed setup', async () => {
    const c = controller({
      phantom: true,
      starting: true,
      commandsLoaded: false,
      commands: [],
    });
    c.pendingPrompt = {
      message: '/reload',
      draft: ' /reload ',
      command: true,
      optimisticId: 'unused',
      stateRequestId: '',
      messagesRequestId: '',
      selectedModelProvider: '',
      selectedModelId: '',
      selectedModelName: '',
      selectedEffort: 'off',
      settingsRequestId: '',
      settingsStep: '',
    };
    const bootstrapCommands = nextRequestId('bootstrap-commands');
    await rpc(c, { id: bootstrapCommands, type: 'get_commands' });
    const stateId = nextRequestId('bootstrap-state');
    c.pendingPrompt.stateRequestId = stateId;
    await rpc(c, { id: stateId, type: 'get_state' });
    await handleResponse(c, {
      id: stateId,
      command: 'get_state',
      success: true,
      data: {
        sessionId: c.sessionId,
        sessionFile: c.sessionPath,
        isStreaming: false,
      },
    });
    const messagesId = c.pendingPrompt!.messagesRequestId;
    await handleResponse(c, {
      id: messagesId,
      command: 'get_messages',
      success: true,
      data: { messages: [] },
    });
    expect(c.pendingPrompt).toBeUndefined();
    expect(c.starting).toBe(false);
    expect(c.reload?.phase).toBe('discovery');
    await handleResponse(c, {
      id: bootstrapCommands,
      command: 'get_commands',
      success: true,
      data: { commands: [helper] },
    });
    expect(c.reload?.phase).toBe('discovery');
    await answer(c, 'get_commands', { commands: [helper] });
    expect(c.reload?.phase).toBe('dispatch');
    expect(c.messages).toEqual([]);
    expect(c.lastUserMessageAt).toBe(0);
    expect(JSON.stringify(vi.mocked(invoke).mock.calls)).not.toContain(
      'set_model',
    );
    expect(JSON.stringify(vi.mocked(invoke).mock.calls)).not.toContain(
      'set_thinking_level',
    );
  });

  it('retains private extension failures even when Pi acknowledges handling', async () => {
    const c = controller();
    await startReload(c, '/reload');
    await handleRpc(c, {
      type: 'extension_error',
      extensionPath: 'private/path',
      event: `command:${TAU_RELOAD_COMMAND}`,
      error: 'private failure',
    });
    await acknowledge(c);
    await finish(c);
    expect(c.draft).toBe('/reload');
    expect(JSON.stringify(c.localErrors)).not.toContain('private failure');
    expect(c.reload).toBeUndefined();
  });

  it('times out to unknown, rejects mutations, and reconciles late completion without replay', async () => {
    const c = controller();
    await startReload(c, '/reload');
    c.draft = 'typed meanwhile';
    await vi.advanceTimersByTimeAsync(30_001);
    expect(c.reload?.status).toBe('unknown');
    expect(c.draft).toContain('typed meanwhile');
    await expect(
      rpc(c, { id: nextRequestId('model'), type: 'set_model' }),
    ).rejects.toThrow();
    await acknowledge(c);
    await finish(c);
    expect(c.reload).toBeUndefined();
    expect(c.draft).toContain('typed meanwhile');
  });

  it('keeps ownership after uncertain transport delivery and reconciles a late acknowledgement', async () => {
    const c = controller();
    vi.mocked(invoke).mockRejectedValueOnce(new Error('write outcome unknown'));
    await startReload(c, '/reload');
    expect(c.reload?.status).toBe('unknown');
    expect(c.draft).toBe('/reload');
    await expect(
      rpc(c, { id: nextRequestId('model'), type: 'set_model' }),
    ).rejects.toThrow();
    await acknowledge(c);
    await finish(c);
    expect(c.reload).toBeUndefined();
    expect(c.draft).toBe('/reload');
  });

  it('restores a rejected command without replacing new text or retrying it', async () => {
    const c = controller();
    await startReload(c, ' /reload ');
    const id = c.reload!.requestId;
    c.draft = 'new draft';
    await handleResponse(c, {
      id,
      command: 'prompt',
      success: false,
      error: 'private error',
    });
    expect(c.reload).toBeUndefined();
    expect(c.draft).toBe(' /reload \n\nnew draft');
    expect(JSON.stringify(c.feedback)).not.toContain('private error');
    await vi.advanceTimersByTimeAsync(31_000);
    expect(c.reload).toBeUndefined();
  });

  it('fails discovery before dispatch without holding an unknown mutation lock', async () => {
    const c = controller({ commandsLoaded: false });
    await startReload(c, '/reload');
    await answer(c, 'get_commands', {}, false);
    expect(c.reload).toBeUndefined();
    expect(c.draft).toBe('/reload');
  });

  it('keeps a failed refresh fenced and owned until recovery', async () => {
    const c = controller();
    await startReload(c, '/reload');
    await acknowledge(c);
    await answer(c, 'get_state', {}, false);
    expect(c.reload?.status).toBe('unknown');
    expect(c.draft).toBe('/reload');
  });

  it('ignores stale catalogue reads after refresh and permits newer recovery reads', async () => {
    const c = controller();
    const old = nextRequestId('old-models');
    await rpc(c, { id: old, type: 'get_available_models' });
    await startReload(c, '/reload');
    await acknowledge(c);
    await finish(c);
    await handleResponse(c, {
      id: old,
      command: 'get_available_models',
      success: true,
      data: { models: [{ provider: 'stale', id: 'stale' }] },
    });
    expect(c.models[0]?.id).toBe('new');
    const fresh = nextRequestId('fresh-models');
    await rpc(c, { id: fresh, type: 'get_available_models' });
    await handleResponse(c, {
      id: fresh,
      command: 'get_available_models',
      success: true,
      data: { models: [{ provider: 'next', id: 'next' }] },
    });
    expect(c.models[0]?.id).toBe('next');
  });

  it('preserves failed model setup through authoritative reload state and does not send a retained draft', async () => {
    const failure: NonNullable<SessionController['setupFailure']> = {
      step: 'model',
      provider: 'missing',
      modelId: 'wanted',
      effort: 'high',
      message: 'Model setup failed',
      recovering: false,
    };
    const c = controller({
      setupFailure: failure,
      draft: 'retained ordinary draft',
    });
    await startReload(c, '/reload');
    await acknowledge(c);
    await finish(c);
    expect(c.setupFailure).toEqual(failure);
    expect(c.draft).toBe('retained ordinary draft');
    expect(c.pendingPrompt).toBeUndefined();
    const requests = JSON.stringify(vi.mocked(invoke).mock.calls);
    expect(requests).not.toContain('set_model');
    expect(requests).not.toContain('set_thinking_level');
    expect(requests).not.toContain('retained ordinary draft');
  });

  it('keeps supported reload-time dialogs usable beyond the work deadline', async () => {
    const c = controller();
    await startReload(c, '/reload');
    await handleRpc(c, {
      type: 'extension_ui_request',
      method: 'confirm',
      id: 'reload-confirm',
      title: 'Confirm resource setup',
    });
    expect(state.extensionDialogs).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(60_001);
    expect(c.reload?.status).toBe('reloading');
    await expect(
      rpc(c, {
        type: 'extension_ui_response',
        id: 'reload-confirm',
        confirmed: true,
      }),
    ).resolves.toBeUndefined();
    state.extensionDialogs.splice(0);
    await acknowledge(c);
    await finish(c);
    expect(c.reload).toBeUndefined();
  });

  it('keeps background reload ownership across idle eviction and leaves another draft alone', async () => {
    const { releaseIdleRuntimes } = await import('./lifecycle');
    const c = controller();
    const other = controller({
      key: 'other',
      runtimeId: 'other',
      draft: 'other draft',
    });
    state.activeControllerKey = other.key;
    await startReload(c, '/reload');
    for (let index = 0; index < 10; index += 1)
      controller({ key: `idle-${index}`, runtimeId: `idle-${index}` });
    releaseIdleRuntimes();
    expect(c.generation).toBe(1);
    expect(c.reload).toBeDefined();
    await acknowledge(c);
    await finish(c);
    expect(other.draft).toBe('other draft');
    expect(state.activeControllerKey).toBe(other.key);
  });

  it('clears timers on process exit and fences late completion from a newer operation', async () => {
    const { handleBridgeEvent } = await import('./index');
    const c = controller();
    await startReload(c, '/reload');
    const id = c.reload!.requestId;
    await handleBridgeEvent({
      runtimeId: c.runtimeId,
      generation: c.generation,
      kind: 'exited',
      code: 1,
    });
    expect(c.reload).toBeUndefined();
    expect(c.draft).toBe('/reload');
    c.generation = 2;
    c.ready = true;
    c.runtimeDisconnected = false;
    await startReload(c, '/reload');
    const next = c.reload;
    await handleResponse(c, { id, command: 'prompt', success: true });
    expect(c.reload).toBe(next);
    expect(c.reload?.phase).toBe('dispatch');
  });

  it('reconciles reload-hook identity replacement without losing a newly opened dialog', async () => {
    const c = controller();
    await startReload(c, '/reload');
    await handleRpc(c, {
      type: 'extension_ui_request',
      method: 'input',
      id: 'new-hook-dialog',
      title: 'New runtime input',
    });
    await acknowledge(c);
    await answer(c, 'get_state', {
      sessionId: 'replacement',
      sessionFile: '/tmp/project/replacement.json',
      isStreaming: false,
    });
    expect(c.sessionId).toBe('replacement');
    expect(state.extensionDialogs).toHaveLength(1);
    await answer(c, 'get_commands', { commands: [helper] });
    await answer(c, 'get_available_models', { models: [] });
    await answer(c, 'get_available_thinking_levels', { levels: ['off'] });
    await answer(c, 'get_messages', { messages: [] });
    expect(c.reload).toBeUndefined();
    expect(state.extensionDialogs[0]?.requestId).toBe('new-hook-dialog');
  });

  it('does not dispatch if Pi becomes busy before discovery completes', async () => {
    const c = controller({ commandsLoaded: false });
    await startReload(c, '/reload');
    c.compacting = true;
    await answer(c, 'get_commands', { commands: [helper] });
    expect(c.reload).toBeUndefined();
    expect(c.draft).toBe('/reload');
  });
});
