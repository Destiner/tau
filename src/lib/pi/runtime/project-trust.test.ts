import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import tauReloadHelper from '../../../../src-tauri/pi-extensions/tau-reload';
import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  activeController,
  inheritControllerSettings,
  nextRequestId,
  state,
  type SessionController,
} from '../../../composables/state';
import { invokeTraced } from '../../telemetry';
import type { PiBridgeEvent } from '../bridge';

import { handleBridgeEvent } from './events';
import { rpc, startController, stopControllerProcess } from './lifecycle';
import {
  effectiveProjectTrust,
  markProjectTrustGranted,
  projectTrustStatusKey,
} from './project-trust';

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

const first = { sessionId: 'session-1', sessionFile: '/sessions/first.jsonl' };
const second = {
  sessionId: 'session-2',
  sessionFile: '/sessions/second.jsonl',
};
const cwd = '/a project/with "quotes"';

function mount(overrides: Partial<SessionController> = {}): SessionController {
  state.controllers.push(
    sessionControllerFixture({
      generation: 1,
      sessionPath: first.sessionFile,
      ...overrides,
    }),
  );
  return state.controllers.at(-1)!;
}

function bridge(
  controller: SessionController,
  event: Omit<PiBridgeEvent, 'runtimeId'>,
): Promise<void> {
  return handleBridgeEvent({ runtimeId: controller.runtimeId, ...event });
}

function status(
  controller: SessionController,
  payload: unknown,
  generation = controller.generation,
  statusKey = projectTrustStatusKey,
): Promise<void> {
  return bridge(controller, {
    kind: 'rpc',
    generation,
    line: JSON.stringify({
      type: 'extension_ui_request',
      id: 'status-id',
      method: 'setStatus',
      statusKey,
      statusText:
        typeof payload === 'string' ? payload : JSON.stringify(payload),
    }),
  });
}

function report(
  trusted: boolean,
  identity: { sessionId: string; sessionFile: string | null } = first,
): Record<string, unknown> {
  return { version: 1, trusted, cwd, ...identity };
}

async function readState(
  controller: SessionController,
  identity: { sessionId: string; sessionFile?: string } = first,
  correlated = true,
): Promise<void> {
  const id = nextRequestId('trust-test-state');
  if (correlated) await rpc(controller, { id, type: 'get_state' });
  await bridge(controller, {
    kind: 'rpc',
    generation: controller.generation,
    line: JSON.stringify({
      type: 'response',
      command: 'get_state',
      id,
      success: true,
      data: identity,
    }),
  });
}

beforeEach(() => {
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.workspace = null;
  state.activeControllerKey = '';
  vi.mocked(invoke).mockClear();
  vi.mocked(invokeTraced).mockReset();
  vi.mocked(invokeTraced).mockResolvedValue(undefined);
});

describe('effective project trust', () => {
  it('buffers an early startup report and confirms the Pi identity of a phantom', async () => {
    const controller = mount({
      generation: 0,
      starting: true,
      phantom: true,
      sessionId: 'tau-phantom',
      sessionPath: '',
    });
    await status(controller, report(false), 2);
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    await bridge(controller, { kind: 'started', generation: 2 });
    await readState(controller, first, false);
    expect(effectiveProjectTrust(controller)).toBeUndefined();

    await readState(controller);
    expect(controller.sessionId).toBe('tau-phantom');
    expect(effectiveProjectTrust(controller)).toEqual({
      ...first,
      trusted: false,
      cwd,
      generation: 2,
    });
    await status(controller, report(false), 2, 'another-extension');
    expect(state.extensionDialogs).toEqual([]);
    expect(controller.messages).toEqual([]);
  });

  it.each([2, 3])(
    'only accepts early reports for the generation acknowledged by start_pi (%i)',
    async (generation) => {
      const controller = mount({
        generation: 0,
        phantom: true,
        sessionId: 'phantom',
        sessionPath: '',
      });
      vi.mocked(invokeTraced).mockImplementation(async (command) => {
        if (command !== 'start_pi') return undefined as never;
        await status(
          controller,
          report(false, { ...first, sessionFile: null }),
          2,
        );
        return generation as never;
      });
      await startController(controller, {
        path: controller.projectPath,
        workingDirectory: cwd,
        name: 'Project',
        selected: true,
        collapsed: false,
        sessions: [],
      });
      expect(effectiveProjectTrust(controller)).toBeUndefined();
      await readState(controller, { sessionId: first.sessionId });
      expect(effectiveProjectTrust(controller)?.trusted).toBe(
        generation === 2 ? false : undefined,
      );
    },
  );

  it('accepts a late matching report but does not expose malformed or unmatched reports', async () => {
    const controller = mount();
    await readState(controller);
    for (const invalid of [
      '[',
      { ...report(false), version: 2 },
      { ...report(false), trusted: 'false' },
      { ...report(false), cwd: 'relative' },
      { ...report(false), cwd: '/invalid\0path' },
      { ...report(false), sessionId: '' },
      { ...report(false), sessionFile: undefined },
    ]) {
      await status(controller, invalid);
      expect(effectiveProjectTrust(controller)).toBeUndefined();
    }
    await status(controller, report(false), 1, 'another-extension');
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    for (const identity of [
      second,
      { ...first, sessionFile: second.sessionFile },
    ]) {
      await status(controller, report(false, identity));
      expect(effectiveProjectTrust(controller)).toBeUndefined();
      await readState(controller);
      expect(effectiveProjectTrust(controller)).toBeUndefined();
    }
    await status(controller, report(true));
    expect(effectiveProjectTrust(controller)?.trusted).toBe(true);
    await status(controller, { ...report(false), version: 0 });
    expect(effectiveProjectTrust(controller)?.trusted).toBe(true);
  });

  it('isolates background reports and confirms replacements without reusing the predecessor', async () => {
    const controller = mount();
    const background = mount({
      key: 'background',
      runtimeId: 'background-runtime',
    });
    state.activeControllerKey = controller.key;
    await status(controller, report(true));
    await readState(controller);
    await status(background, report(false));
    await readState(background);
    expect(effectiveProjectTrust(activeController.value)?.trusted).toBe(true);
    expect(effectiveProjectTrust(background)?.trusted).toBe(false);

    await status(background, report(true, second), 0);
    expect(effectiveProjectTrust(background)?.trusted).toBe(false);
    await status(background, { ...report(true, second), version: 0 });
    expect(effectiveProjectTrust(background)?.trusted).toBe(false);

    await status(background, report(true, second));
    expect(effectiveProjectTrust(background)).toBeUndefined();
    await status(background, report(false));
    expect(effectiveProjectTrust(background)).toBeUndefined();
    await readState(background, second, false);
    expect(effectiveProjectTrust(background)).toBeUndefined();
    expect(effectiveProjectTrust(activeController.value)?.trusted).toBe(true);

    await readState(background, second);
    expect(effectiveProjectTrust(background)).toMatchObject({
      ...second,
      trusted: true,
    });
    await status(background, report(false));
    expect(effectiveProjectTrust(background)?.trusted).toBe(true);
    const third = { sessionId: 'third', sessionFile: '/sessions/third.jsonl' };
    await readState(background, third);
    expect(effectiveProjectTrust(background)).toBeUndefined();
    await readState(background, first);
    expect(effectiveProjectTrust(background)).toBeUndefined();
    expect(effectiveProjectTrust(activeController.value)?.trusted).toBe(true);
  });

  it('does not let a predecessor report hide the current report or overwrite a queued successor', async () => {
    const controller = mount();
    await status(controller, report(false));
    await readState(controller);
    await status(controller, report(false, second));
    await readState(controller, second);
    await readState(controller, second);

    await status(controller, report(true));
    expect(effectiveProjectTrust(controller)).toMatchObject({
      ...second,
      trusted: false,
    });
    const third = { sessionId: 'third', sessionFile: '/sessions/third.jsonl' };
    await status(controller, report(true, third));
    await status(controller, report(false));
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    await readState(controller, third);
    expect(effectiveProjectTrust(controller)).toMatchObject({
      ...third,
      trusted: true,
    });
  });

  it('accepts a return to the preceding session once a correlated read confirms it', async () => {
    const controller = mount();
    await status(controller, report(false));
    await readState(controller);
    await status(controller, report(true, second));
    await readState(controller, second);

    await status(controller, report(false));
    await readState(controller, first, false);
    expect(effectiveProjectTrust(controller)?.trusted).toBe(true);
    await readState(controller);
    expect(effectiveProjectTrust(controller)).toMatchObject({
      ...first,
      trusted: false,
    });
  });

  it('discards a pending successor if the next correlated read still reports the predecessor', async () => {
    const controller = mount();
    await status(controller, report(false));
    await readState(controller);
    await status(controller, report(true, second));
    expect(effectiveProjectTrust(controller)).toBeUndefined();

    await readState(controller);
    expect(effectiveProjectTrust(controller)?.trusted).toBe(false);
    await readState(controller, second);
    expect(effectiveProjectTrust(controller)).toBeUndefined();
  });

  it('does not reuse an early report across an error, exit or generation change', async () => {
    const controller = mount({ generation: 0, starting: true });
    await status(controller, report(false), 1);
    await bridge(controller, { kind: 'error', generation: 1 });
    await bridge(controller, { kind: 'started', generation: 1 });
    await readState(controller);
    await status(controller, report(false), 1);
    expect(effectiveProjectTrust(controller)).toBeUndefined();

    await bridge(controller, { kind: 'started', generation: 2 });
    await readState(controller);
    await status(controller, report(false), 1);
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    await status(controller, report(false), 2);
    expect(effectiveProjectTrust(controller)?.trusted).toBe(false);
    await bridge(controller, { kind: 'exited', generation: 2, code: 0 });
    expect(effectiveProjectTrust(controller)).toBeUndefined();
  });

  it('hides reports on disconnection and waits for the recovered process identity', async () => {
    const controller = mount();
    await status(controller, report(false));
    await readState(controller);
    await bridge(controller, { kind: 'error', generation: 1 });
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    await status(controller, report(false));
    expect(effectiveProjectTrust(controller)).toBeUndefined();

    controller.recoveringRuntime = true;
    controller.starting = true;
    await status(controller, report(true), 2);
    await bridge(controller, { kind: 'started', generation: 2 });
    await readState(controller);
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    controller.runtimeDisconnected = false;
    controller.recoveringRuntime = false;
    expect(effectiveProjectTrust(controller)?.trusted).toBe(true);
  });

  it('hides the report as soon as an eviction stop begins and does not revive it', async () => {
    const controller = mount();
    await status(controller, report(false));
    await readState(controller);
    let finishStop: (() => void) | undefined;
    vi.mocked(invokeTraced).mockReturnValueOnce(
      new Promise<void>((resolve) => {
        finishStop = resolve;
      }),
    );
    const stopping = stopControllerProcess(controller, undefined, false);
    expect(effectiveProjectTrust(controller)).toBeUndefined();
    await status(controller, report(false));
    finishStop?.();
    await stopping;
    expect(effectiveProjectTrust(controller)).toBeUndefined();
  });
});

it('does not inherit commands from a launch already in flight when trust was saved', async () => {
  const projectPath = '/trust-granted-while-starting';
  const controller = mount({ projectPath, generation: 0 });
  const other = mount({
    key: 'other',
    runtimeId: 'other-runtime',
    projectPath: '/another-account',
  });
  other.commands = [{ name: 'other', source: 'extension' }];
  other.commandsLoaded = true;
  vi.mocked(invokeTraced).mockImplementation(async (command) => {
    if (command !== 'start_pi') return undefined as never;
    markProjectTrustGranted(projectPath);
    return 2 as never;
  });
  await startController(controller, {
    path: projectPath,
    workingDirectory: cwd,
    name: 'Project',
    selected: true,
    collapsed: false,
    sessions: [],
  });
  await bridge(controller, {
    kind: 'rpc',
    generation: controller.generation,
    line: JSON.stringify({
      type: 'response',
      command: 'get_commands',
      success: true,
      data: { commands: [{ name: 'old', source: 'extension' }] },
    }),
  });
  expect(controller.commandsLoaded).toBe(true);
  const affectedSibling = sessionControllerFixture({
    key: 'affected-sibling',
    projectPath,
  });
  inheritControllerSettings(affectedSibling, controller);
  expect(affectedSibling.commandsLoaded).toBe(false);
  const otherSibling = sessionControllerFixture({
    key: 'other-sibling',
    projectPath: other.projectPath,
  });
  inheritControllerSettings(otherSibling, other);
  expect(otherSibling.commands).toEqual(other.commands);
  expect(otherSibling.commandsLoaded).toBe(true);
});

describe('private Pi helper', () => {
  it('does not require trust APIs to register the reload command', async () => {
    const registerCommand =
      vi.fn<Parameters<typeof tauReloadHelper>[0]['registerCommand']>();
    tauReloadHelper({ registerCommand });
    const [, command] = registerCommand.mock.calls[0]!;
    const reload = vi.fn(async () => undefined);
    await command.handler('', { isIdle: () => true, reload });
    expect(reload).toHaveBeenCalledOnce();
  });

  it('emits the versioned status only when effective trust is available', () => {
    const on =
      vi.fn<NonNullable<Parameters<typeof tauReloadHelper>[0]['on']>>();
    tauReloadHelper({ on, registerCommand: vi.fn() });
    const [event, handler] = on.mock.calls[0]!;
    expect(event).toBe('session_start');
    const setStatus = vi.fn();
    handler({}, { cwd, ui: { setStatus } });
    expect(setStatus).not.toHaveBeenCalled();
    handler(
      {},
      {
        cwd,
        ui: { setStatus },
        isProjectTrusted: () => false,
        sessionManager: {
          getSessionId: () => first.sessionId,
          getSessionFile: () => first.sessionFile,
        },
      },
    );
    expect(setStatus).toHaveBeenCalledExactlyOnceWith(
      projectTrustStatusKey,
      expect.any(String),
    );
    expect(JSON.parse(String(setStatus.mock.calls[0]?.[1]))).toEqual(
      report(false),
    );
  });
});
