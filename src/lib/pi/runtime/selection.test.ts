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
    const telemetry = await import('../../telemetry');
    const { persistProjectSelection } = await import('./index');
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
    expect(controller.feedback).toEqual([]);
    expect(controller.localErrors).toEqual([]);
  });

  it('persists a registered session selection', async () => {
    const telemetry = await import('../../telemetry');
    const { persistProjectSelection } = await import('./index');
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
    const telemetry = await import('../../telemetry');
    const { persistProjectSelection } = await import('./index');
    const controller = makeController();
    setWorkspaceSessions(controller, true);
    vi.mocked(telemetry.invokeTraced).mockRejectedValueOnce(
      new Error('Native persistence failed'),
    );

    await persistProjectSelection(controller.projectPath, controller);

    expect(feedbackMessage(controller)).toBe(
      'This selection could not be saved. Select it again.',
    );
  });
});
