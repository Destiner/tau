import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../tests/support/session-controller';

import type {
  ProjectSummary,
  SessionController,
  SessionSummary,
} from './state';

beforeEach(() => {
  vi.resetModules();
});

const testController = sessionControllerFixture;

describe('session tooltip source', () => {
  it('keeps indentation, blank lines and Unicode within 240 characters', async () => {
    const { sessionTitleMarkdown, tooltipTitleMarkdown, draftTitle } =
      await import('./state');
    const source = '  **Hello**\r\n\r\n\t```\n🚀';
    expect(sessionTitleMarkdown(source)).toBe(source);
    expect(draftTitle(source)).toBe('**Hello** ``` 🚀');
    expect(
      tooltipTitleMarkdown({
        title: 'Compact',
        titleMarkdown: source,
      } as SessionSummary),
    ).toBe(source);
    expect(tooltipTitleMarkdown({ title: 'Fallback' } as SessionSummary)).toBe(
      'Fallback',
    );
    expect(sessionTitleMarkdown(' \n\t')).toBe('New Session');
    expect(Array.from(sessionTitleMarkdown('🚀'.repeat(241)))).toHaveLength(
      240,
    );
    expect(sessionTitleMarkdown('x'.repeat(239) + '**more')).toBe(
      'x'.repeat(239) + '*',
    );
  });

  it('updates phantom preview with drafts without altering the draft', async () => {
    const { state, draft, createPhantomSession } = await import('./state');
    const controller = testController({ key: 'draft-owner', phantom: true });
    state.controllers = [controller];
    state.activeControllerKey = controller.key;
    const session = createPhantomSession(
      controller.projectPath,
      controller.key,
    );
    state.ephemeralSessions = [session];
    draft.value = '  **First**\n\n  - next';
    expect(controller.draft).toBe('  **First**\n\n  - next');
    expect(session.titleMarkdown).toBe(controller.draft);
    draft.value = '   ';
    expect(session.titleMarkdown).toBe('New Session');
  });
});

describe('classifyControllerLifecycle', () => {
  it('classifies idle, ready, streaming and precedence of active states', async () => {
    const { classifyControllerLifecycle } = await import('./state');
    const flags: Partial<SessionController> = {};
    for (const [patch, expected] of [
      [{}, 'idle'],
      [{ ready: true }, 'ready'],
      [{ streaming: true }, 'working'],
      [{ working: true, syncing: true }, 'syncing'],
      [{ stopping: true }, 'stopping'],
      [{ starting: true }, 'starting'],
      [{ connectingRemote: true }, 'connecting'],
    ] as const) {
      Object.assign(flags, patch);
      expect(classifyControllerLifecycle(testController(flags))).toBe(expected);
    }
  });
});

describe('sessionWorkInProgress', () => {
  it('counts each active lifecycle flag as work', async () => {
    const { sessionWorkInProgress, state } = await import('./state');
    state.extensionDialogs = [];
    for (const flags of [
      { connectingRemote: true },
      { starting: true },
      { stopping: true },
      { syncing: true },
      { working: true },
      { streaming: true },
      { compacting: true },
      { compactionReconciliationPending: true },
      { promptSubmitting: true },
    ]) {
      expect(sessionWorkInProgress(testController(flags))).toBe(true);
    }
  });

  it('counts prompt admission and extension input as pending work', async () => {
    const { sessionWorkInProgress, state } = await import('./state');
    const pending = testController({
      pendingPrompt: {
        message: 'Prompt',
        draft: 'Prompt',
        optimisticId: 'optimistic-1',
        command: false,
        stateRequestId: 'state-1',
        messagesRequestId: 'messages-1',
        selectedModelProvider: 'provider',
        selectedModelId: 'model',
        selectedModelName: 'Model',
        selectedEffort: 'medium',
        settingsRequestId: 'settings-1',
        settingsStep: '',
      },
    });
    const submitted = testController({
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Prompt',
        draft: 'Prompt',
        accepted: false,
      },
    });
    const waitingForInput = testController({ key: 'dialog-controller' });
    state.extensionDialogs = [
      {
        key: 'dialog-1',
        requestId: 'request-1',
        method: 'confirm',
        title: 'Continue?',
        draft: '',
        submitting: false,
        error: '',
        controllerKey: waitingForInput.key,
        runtimeId: waitingForInput.runtimeId,
        generation: 1,
        projectName: 'Project',
        sessionName: 'Session',
      },
    ];

    expect(sessionWorkInProgress(pending)).toBe(true);
    expect(sessionWorkInProgress(submitted)).toBe(true);
    expect(sessionWorkInProgress(waitingForInput)).toBe(true);
  });

  it('does not count idle, ready, or disposed controllers', async () => {
    const { sessionWorkInProgress, state } = await import('./state');
    state.extensionDialogs = [];

    expect(sessionWorkInProgress(testController())).toBe(false);
    expect(sessionWorkInProgress(testController({ ready: true }))).toBe(false);
    expect(
      sessionWorkInProgress(testController({ working: true, disposed: true })),
    ).toBe(false);
  });

  it('counts distinct pending sessions across foreground and background controllers', async () => {
    const { inProgressSessionCount, state } = await import('./state');
    state.extensionDialogs = [];
    state.controllers = [
      testController({ key: 'foreground', working: true }),
      testController({ key: 'replacement', streaming: true }),
      testController({
        key: 'background',
        sessionId: 'session-2',
        projectPath: '/tmp/other',
        stopping: true,
      }),
      testController({ key: 'idle', sessionId: 'session-3', ready: true }),
    ];

    expect(inProgressSessionCount.value).toBe(2);
  });
});

describe('sessionTooltipStatus', () => {
  const session: SessionSummary = {
    id: 'session-1',
    path: '/tmp/project/session.jsonl',
    title: 'Session',
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: 1,
    archived: false,
    selected: false,
  };
  const project: ProjectSummary = {
    path: '/tmp/project',
    name: 'Project',
    workingDirectory: '/tmp/project',
    collapsed: false,
    selected: false,
    sessions: [session],
  };

  it('prioritizes existing session indicator statuses', async () => {
    const { sessionTooltipStatus, state } = await import('./state');
    state.extensionDialogs = [];
    for (const [controllerState, expected] of [
      [{ working: true, draft: 'Unsent', unread: true }, 'Working'],
      [{ draft: 'Unsent', unread: true }, 'Draft'],
      [{ unread: true }, 'Unread'],
    ] as const) {
      state.controllers = [testController(controllerState)];
      expect(sessionTooltipStatus(project, session)).toBe(expected);
    }
    state.controllers = [testController({ draft: '  \n  ' })];
    expect(sessionTooltipStatus(project, session)).toBe('Idle');
  });

  it('prioritizes extension input and archived status, then falls back without a controller', async () => {
    const { sessionTooltipStatus, sessionLastActive, state } =
      await import('./state');
    const controller = testController({ working: true, draft: 'Unsent' });
    state.controllers = [controller];
    state.extensionDialogs = [
      {
        key: 'dialog-1',
        requestId: 'request-1',
        method: 'confirm',
        title: 'Continue?',
        draft: '',
        submitting: false,
        error: '',
        controllerKey: controller.key,
        runtimeId: controller.runtimeId,
        generation: controller.generation,
        projectName: 'Project',
        sessionName: 'Session',
      },
    ];

    expect(sessionTooltipStatus(project, session)).toBe('Unread');
    expect(sessionLastActive(project, session)).toBe('');
    expect(sessionTooltipStatus(project, { ...session, archived: true })).toBe(
      'Archived',
    );

    state.controllers = [];
    const controllers = state.controllers;
    const projectBefore = { ...project, sessions: [...project.sessions] };
    const sessionBefore = { ...session };

    expect(sessionTooltipStatus(project, session)).toBe('Idle');
    expect(state.controllers).toBe(controllers);
    expect(project).toEqual(projectBefore);
    expect(session).toEqual(sessionBefore);
  });
});

describe('sessionLastActive', () => {
  const session: SessionSummary = {
    id: 'session-1',
    path: '/tmp/project/session.jsonl',
    title: 'Session',
    lastActive: '2h',
    lastUserMessageAt: 0,
    sortAt: 100,
    archived: false,
    selected: false,
  };
  const project: ProjectSummary = {
    path: '/tmp/project',
    name: 'Project',
    workingDirectory: '/tmp/project',
    collapsed: false,
    selected: false,
    sessions: [session],
  };

  it('hides working recency and restores it when work ends', async () => {
    const { sessionLastActive, state } = await import('./state');
    const controller = testController({ working: true });
    state.controllers = [controller];
    state.ephemeralSessions = [];

    expect(sessionLastActive(project, session)).toBe('');
    expect(sessionLastActive(project, { ...session, lastActive: 'now' })).toBe(
      '',
    );
    controller.working = false;
    expect(sessionLastActive(project, session, 100 + 2 * 3_600_000)).toBe('2h');
  });

  it('ages saved labels with numeric recency, including equal controller timestamps', async () => {
    const { sessionLastActive, sessionSortAt, state } = await import('./state');
    const at = 1_800_000_000_000;
    const recent = {
      ...session,
      lastActive: 'now',
      lastUserMessageAt: at,
      sortAt: at,
    };
    const before = { ...recent };
    state.controllers = [];
    expect(sessionLastActive(project, recent, at)).toBe('now');
    expect(sessionLastActive(project, recent, at + 61_000)).toBe('1m');
    state.controllers = [testController({ lastUserMessageAt: at })];
    expect(sessionLastActive(project, recent, at + 3_600_000)).toBe('1h');
    state.controllers[0]!.lastUserMessageAt = at + 120_000;
    expect(sessionLastActive(project, recent, at + 180_000)).toBe('1m');
    state.controllers[0]!.lastUserMessageAt = at - 120_000;
    expect(sessionLastActive(project, recent, at + 180_000)).toBe('3m');
    expect(sessionSortAt(project.path, recent)).toBe(at);
    expect(recent).toEqual(before);
  });

  it('retains numeric sort fallback, textual zero fallback and working/ephemeral guards', async () => {
    const { createPhantomSession, sessionLastActive, state } =
      await import('./state');
    const at = 1_800_000_000_000;
    state.controllers = [];
    expect(
      sessionLastActive(
        project,
        { ...session, sortAt: at, lastActive: 'now' },
        at + 61_000,
      ),
    ).toBe('1m');
    expect(sessionLastActive(project, { ...session, sortAt: 0 }, at)).toBe(
      '2h',
    );
    expect(
      sessionLastActive(project, { ...session, sortAt: 0, lastActive: '' }, at),
    ).toBe('');

    const controller = testController({ working: true, lastUserMessageAt: at });
    state.controllers = [controller];
    const active = { ...session, sortAt: at, lastUserMessageAt: at };
    expect(sessionLastActive(project, active, at + 3_600_000)).toBe('');
    controller.working = false;
    expect(sessionLastActive(project, active, at + 3_600_000)).toBe('1h');

    const phantom = createPhantomSession(project.path, controller.key);
    phantom.sortAt = at;
    phantom.lastUserMessageAt = at;
    state.ephemeralSessions = [phantom];
    controller.sessionId = phantom.id;
    controller.messages = [];
    expect(sessionLastActive(project, phantom, at + 61_000)).toBe('');
    controller.draft = 'Draft';
    expect(sessionLastActive(project, phantom, at + 61_000)).toBe('');
    controller.messages = [
      { kind: 'user' } as SessionController['messages'][number],
    ];
    expect(sessionLastActive(project, phantom, at + 61_000)).toBe('1m');
  });

  it.each([
    [0, 'now'],
    [60_000, '1m'],
    [3_600_000, '1h'],
    [86_400_000, '1d'],
    [604_800_000, '1w'],
    [31_536_000_000, '1y'],
    [-60_000, 'now'],
  ])('formats elapsed %i ms as %s', async (elapsed, expected) => {
    const { relativeTimestamp } = await import('./state');
    expect(
      relativeTimestamp(1_800_000_000_000, 1_800_000_000_000 + elapsed),
    ).toBe(expected);
  });
});

describe('expandedRelativeTime', () => {
  it('expands supported relative times and preserves unknown empty input', async () => {
    const { expandedRelativeTime } = await import('./state');
    for (const [value, expected] of [
      ['now', 'just now'],
      ['1m', '1 minute ago'],
      ['2m', '2 minutes ago'],
      ['1h', '1 hour ago'],
      ['2h', '2 hours ago'],
      ['1d', '1 day ago'],
      ['2d', '2 days ago'],
      ['1w', '1 week ago'],
      ['3w', '3 weeks ago'],
      ['1y', '1 year ago'],
      ['2y', '2 years ago'],
      ['', ''],
    ] as const) {
      expect(expandedRelativeTime(value)).toBe(expected);
    }
  });
});

describe('canArchiveSession', () => {
  const session: SessionSummary = {
    id: 'session-1',
    path: '/tmp/project/session.jsonl',
    title: 'Session',
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: 1,
    archived: false,
    selected: true,
  };
  const project: ProjectSummary = {
    path: '/tmp/project',
    name: 'Project',
    workingDirectory: '/tmp/project',
    collapsed: false,
    selected: true,
    sessions: [session],
  };

  it('allows registered sessions only when their controller is idle or ready', async () => {
    const { canArchiveSession, state } = await import('./state');
    state.ephemeralSessions.splice(0);
    state.removingProjectPaths.splice(0);
    state.controllers.splice(0, state.controllers.length, testController());
    expect(canArchiveSession(project, session)).toBe(true);

    state.controllers[0]!.ready = true;
    for (const flags of [
      { connectingRemote: true },
      { starting: true },
      { stopping: true },
      { syncing: true },
      { working: true },
      { streaming: true },
    ]) {
      state.controllers.splice(
        0,
        state.controllers.length,
        testController({ ready: true, ...flags }),
      );
      expect(canArchiveSession(project, session)).toBe(false);
    }
  });

  it('continues rejecting ephemeral sessions and project-action lockout', async () => {
    const { canArchiveSession, state } = await import('./state');
    state.controllers.splice(0);
    state.removingProjectPaths.splice(0);
    state.ephemeralSessions.splice(0, state.ephemeralSessions.length, {
      ...session,
      projectPath: project.path,
      controllerKey: 'controller-1',
      createdAt: 1,
      phantom: false,
    });
    expect(canArchiveSession(project, session)).toBe(false);

    state.ephemeralSessions.splice(0);
    state.removingProjectPaths.push('/tmp/removing');
    expect(canArchiveSession(project, session)).toBe(false);
  });
});

describe('fallback feedback', () => {
  it('holds background failures for their controller and allows repeat incidents', async () => {
    const { acknowledgeFeedback, activeFeedback, setControllerError, state } =
      await import('./state');
    const first = testController({ key: 'first', unread: false });
    const second = testController({ key: 'second' });
    state.controllers = [first, second];
    state.workspaceFeedback = [];
    state.activeControllerKey = second.key;

    setControllerError(first, 'The message could not be sent. Try again.');
    expect(activeFeedback.value).toBeUndefined();
    expect(first.unread).toBe(true);

    state.activeControllerKey = first.key;
    expect(activeFeedback.value?.message).toBe(
      'The message could not be sent. Try again.',
    );
    acknowledgeFeedback(activeFeedback.value!);
    expect(activeFeedback.value).toBeUndefined();

    setControllerError(first, 'The message could not be sent. Try again.');
    expect(first.feedback).toHaveLength(2);
    expect(activeFeedback.value?.id).not.toBe(first.feedback[0]?.id);
  });

  it('coalesces one runtime failure cascade and reopens remote recovery', async () => {
    const {
      acknowledgeFeedback,
      activeFeedback,
      reopenRemoteFeedback,
      setControllerError,
      state,
    } = await import('./state');
    const controller = testController({ key: 'remote' });
    state.controllers = [controller];
    state.workspaceFeedback = [];
    state.activeControllerKey = controller.key;
    const stored = state.controllers[0]!;

    setControllerError(stored, 'The connection was lost.');
    const incidentId = activeFeedback.value?.id;
    expect(activeFeedback.value).toMatchObject({
      title: 'Remote Connection Lost',
      action: 'reconnect',
      message: 'The connection was lost.',
    });
    setControllerError(
      stored,
      'The remote connection failed. Try reconnecting again.',
    );

    expect(stored.feedback).toHaveLength(1);
    expect(activeFeedback.value).toMatchObject({
      id: incidentId,
      action: 'reconnect',
      message: 'The remote connection failed. Try reconnecting again.',
    });

    acknowledgeFeedback(activeFeedback.value!);
    expect(activeFeedback.value).toBeUndefined();
    reopenRemoteFeedback(stored);
    expect(activeFeedback.value?.id).toBe(incidentId);
  });
});

describe('queue recovery identity', () => {
  it('holds an interrupted predecessor draft until that session is reopened', async () => {
    const { ensureController, stashInterruptedQueueDrafts, state } =
      await import('./state');
    const session: SessionSummary = {
      id: 'old',
      path: '/tmp/project/old.jsonl',
      title: 'Old',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: false,
    };
    const project: ProjectSummary = {
      path: '/tmp/project',
      name: 'Project',
      workingDirectory: '/tmp/project',
      collapsed: false,
      selected: true,
      sessions: [session],
    };
    state.controllers.splice(0);
    stashInterruptedQueueDrafts(
      testController({ projectPath: project.path, sessionId: session.id }),
      ['unacknowledged text'],
    );
    const successor = ensureController(project, {
      ...session,
      id: 'new',
      path: '/tmp/project/new.jsonl',
    });
    expect(successor.queueFailedDrafts).toEqual([]);
    const reopened = ensureController(project, session);
    expect(reopened.queueFailedDrafts).toEqual(['unacknowledged text']);
    expect(reopened.queueFeedback).toContain('unsent');
  });

  it('reports acknowledged queue work lost by session replacement without replaying it', async () => {
    const { ensureController, stashInterruptedQueueDrafts, state } =
      await import('./state');
    const session: SessionSummary = {
      id: 'queued',
      path: '/tmp/project/queued.jsonl',
      title: 'Queued',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: false,
    };
    const project: ProjectSummary = {
      path: '/tmp/project',
      name: 'Project',
      workingDirectory: '/tmp/project',
      collapsed: false,
      selected: true,
      sessions: [session],
    };
    state.controllers.splice(0);
    stashInterruptedQueueDrafts(
      testController({ projectPath: project.path, sessionId: session.id }),
      [],
      true,
    );
    const reopened = ensureController(project, session);
    expect(reopened.queueFailedDrafts).toEqual([]);
    expect(reopened.queueFeedback).toContain('were lost');
    expect(reopened.queueFeedback).toContain('not resent');
  });
});
