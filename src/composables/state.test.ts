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

describe('canonical session owner', () => {
  it('prefers a live owner over stopped, disconnected, disposed and stale ephemeral owners', async () => {
    const {
      state,
      controllerForSession,
      claimSessionOwnership,
      createPhantomSession,
    } = await import('./state');
    const stale = testController({
      key: 'stale',
      draft: 'old draft',
      sessionOwnerSequence: 100,
    });
    const owner = testController({
      key: 'owner',
      ready: true,
      draft: 'new draft',
    });
    const disconnected = testController({
      key: 'remote',
      remoteDisconnected: true,
      sessionOwnerSequence: 200,
    });
    const disposed = testController({
      key: 'disposed',
      ready: true,
      disposed: true,
      sessionOwnerSequence: 300,
    });
    state.controllers = [stale, disconnected, disposed, owner];
    const row = createPhantomSession(stale.projectPath, stale.key);
    row.id = stale.sessionId;
    state.ephemeralSessions = [row];
    expect(controllerForSession(owner.projectPath, owner.sessionId)?.key).toBe(
      owner.key,
    );
    claimSessionOwnership(owner);
    expect(owner.draft).toBe('new draft\n\nold draft');
    expect(stale.draft).toBe('');
    expect(disposed.disposed).toBe(true);
  });

  it('reconciles a selected stale view without rebinding a pending dialog', async () => {
    const {
      state,
      claimSessionOwnership,
      activeController,
      activeExtensionDialog,
    } = await import('./state');
    const stale = testController({ key: 'stale', remoteDisconnected: true });
    const owner = testController({ key: 'owner', ready: true, generation: 2 });
    state.controllers = [stale, owner];
    state.activeProjectPath = stale.projectPath;
    state.activeSessionId = stale.sessionId;
    state.activeControllerKey = stale.key;
    state.extensionDialogs = [
      {
        key: 'approval',
        requestId: 'approval',
        method: 'confirm',
        title: 'Approve?',
        draft: 'unfinished answer',
        submitting: false,
        error: '',
        controllerKey: owner.key,
        runtimeId: owner.runtimeId,
        generation: owner.generation,
        projectName: 'Project',
        sessionName: 'Session',
      },
    ];
    claimSessionOwnership(owner);
    expect(state.activeControllerKey).toBe(owner.key);
    expect(activeController.value?.key).toBe(owner.key);
    expect(activeExtensionDialog.value).toMatchObject({
      controllerKey: owner.key,
      generation: 2,
      draft: 'unfinished answer',
    });
    expect(state.extensionDialogs).toHaveLength(1);
    expect(stale.remoteDisconnected).toBe(true);
  });

  it('uses identity adoption rather than navigation when both runtimes are live or starting', async () => {
    const {
      state,
      controllerForSession,
      claimSessionOwnership,
      touchController,
      reconcileSessionOwner,
    } = await import('./state');
    const first = testController({
      key: 'first',
      ready: true,
      draft: 'first draft',
    });
    const second = testController({
      key: 'second',
      starting: true,
      draft: 'second draft',
    });
    state.controllers = [first, second];
    claimSessionOwnership(first);
    claimSessionOwnership(second);
    touchController(first);
    expect(controllerForSession(first.projectPath, first.sessionId)?.key).toBe(
      second.key,
    );
    expect(first.draft).toBe('first draft');
    expect(second.draft).toBe('second draft');
    first.ready = false;
    reconcileSessionOwner(second);
    expect(second.draft).toBe('second draft\n\nfirst draft');
    reconcileSessionOwner(second);
    expect(second.draft).toBe('second draft\n\nfirst draft');
    expect(second.starting).toBe(true);
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

  it('shows current successor reconciliation as Working without changing actual work', async () => {
    const { sessionIndicator, projectIndicator, state } =
      await import('./state');
    const controller = testController({
      generation: 2,
      unread: true,
      syncing: true,
    });
    controller.provisionalReplacement = {
      generation: controller.generation,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
    };
    state.controllers = [controller];
    expect(sessionIndicator(project, session)).toBe('working');
    expect(projectIndicator({ ...project, collapsed: true })).toBe('working');
    expect(controller.working).toBe(false);
    expect(controller.streaming).toBe(false);
    state.extensionDialogs = [
      {
        key: 'successor-input',
        requestId: 'successor-input',
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
    expect(sessionIndicator(project, session)).toBe('new');
    expect(projectIndicator({ ...project, collapsed: true })).toBe('new');
    state.extensionDialogs = [];
    controller.syncing = false;
    controller.postSettlementHydration = true;
    controller.draft = 'Keep draft';
    expect(sessionIndicator(project, session)).toBe('working');
    controller.postSettlementHydration = false;
    expect(sessionIndicator(project, session)).toBe('draft');
    controller.draft = '';
    expect(sessionIndicator(project, session)).toBe('new');
    controller.unread = false;
    expect(sessionIndicator(project, session)).toBe('');
  });

  it('ignores ordinary syncing and stale successor identities', async () => {
    const { sessionIndicator, state } = await import('./state');
    const controller = testController({
      generation: 2,
      unread: true,
      syncing: true,
    });
    state.controllers = [controller];
    expect(sessionIndicator(project, session)).toBe('new');
    const identity = {
      generation: 2,
      sessionId: controller.sessionId,
      sessionPath: controller.sessionPath,
    };
    for (const patch of [
      { generation: 1 },
      { sessionId: 'predecessor' },
      { sessionPath: '/old.jsonl' },
    ]) {
      controller.provisionalReplacement = { ...identity, ...patch };
      expect(sessionIndicator(project, session)).toBe('new');
    }
  });

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
    expect(sessionTooltipStatus(project, session)).toBe('Idle');
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

describe('compaction queue admission', () => {
  it.each(['threshold', 'overflow'] as const)(
    'allows active automatic %s compaction but not pre-run compaction',
    async (compactionReason) => {
      const { controllerCanQueue } = await import('./state');
      const controller = testController({
        ready: true,
        streaming: true,
        compacting: true,
        compactionReason,
        compactionStartedWhileStreaming: true,
      });
      expect(controllerCanQueue(controller)).toBe(true);
      controller.compactionStartedWhileStreaming = false;
      expect(controllerCanQueue(controller)).toBe(false);
      controller.compacting = false;
      expect(controllerCanQueue(controller)).toBe(true);
    },
  );

  it.each(['manual', undefined] as const)(
    'denies compaction reason %s even during an active stream',
    async (compactionReason) => {
      const { controllerCanQueue } = await import('./state');
      expect(
        controllerCanQueue(
          testController({
            ready: true,
            streaming: true,
            compacting: true,
            compactionReason,
            compactionStartedWhileStreaming: true,
          }),
        ),
      ).toBe(false);
    },
  );

  it.each([
    { ready: false },
    { disposed: true },
    { phantom: true },
    { streaming: false },
    { stopping: true },
    { compactionReconciliationPending: true },
    { starting: true },
    { promptSubmitting: true },
    { queueClearing: true },
    { remoteDisconnected: true },
  ] satisfies Partial<SessionController>[])(
    'automatic compaction does not bypass the admission guard %j',
    async (flags) => {
      const { controllerCanQueue } = await import('./state');
      expect(
        controllerCanQueue(
          testController({
            ready: true,
            streaming: true,
            compacting: true,
            compactionReason: 'threshold',
            compactionStartedWhileStreaming: true,
            ...flags,
          }),
        ),
      ).toBe(false);
    },
  );

  it('keeps ordinary compose blocked through queue setup, unacknowledged submission, and compaction', async () => {
    const {
      state,
      canCompose,
      canDraft,
      canQueue,
      compactionRestriction,
      queueBusy,
    } = await import('./state');
    const controller = testController({ ready: true });
    const session: SessionSummary = {
      id: controller.sessionId,
      path: controller.sessionPath,
      title: 'Session',
      lastActive: 'now',
      lastUserMessageAt: 0,
      sortAt: 1,
      archived: false,
      selected: true,
    };
    state.workspace = {
      activeProjectPath: controller.projectPath,
      piPath: '/usr/local/bin/pi',
      projects: [
        {
          path: controller.projectPath,
          name: 'Project',
          workingDirectory: controller.projectPath,
          collapsed: false,
          selected: true,
          sessions: [session],
        },
      ],
    };
    state.controllers = [controller];
    state.activeProjectPath = controller.projectPath;
    state.activeSessionId = controller.sessionId;
    state.activeControllerKey = controller.key;
    const stored = state.controllers[0]!;
    expect(canCompose.value).toBe(true);
    expect(canDraft.value).toBe(true);
    expect(canQueue.value).toBe(false);

    stored.queuePreparing = true;
    expect(queueBusy.value).toBe(true);
    expect(canCompose.value).toBe(false);
    stored.queueSubmissions.push({
      id: 'queue-prompt',
      text: 'same',
      draft: ' same ',
      kind: 'steer',
      version: 0,
      baselineCount: 0,
    });
    stored.queuePreparing = false;
    expect(queueBusy.value).toBe(true);
    expect(canCompose.value).toBe(false);
    stored.queueSubmissions = [];
    expect(queueBusy.value).toBe(false);
    expect(canCompose.value).toBe(true);

    for (const reason of ['threshold', 'overflow'] as const) {
      Object.assign(stored, {
        compacting: true,
        streaming: true,
        compactionReason: reason,
        compactionStartedWhileStreaming: true,
      });
      expect(canQueue.value).toBe(true);
      expect(canCompose.value).toBe(false);
      expect(canDraft.value).toBe(true);
      expect(compactionRestriction.value).toBe('');
      for (const patch of [
        { compactionReason: 'manual' },
        { compactionReason: undefined },
        { compactionStartedWhileStreaming: false },
        { stopping: true },
        { compactionReconciliationPending: true },
      ] satisfies Partial<SessionController>[]) {
        Object.assign(stored, patch);
        expect(canQueue.value).toBe(false);
        expect(canCompose.value).toBe(false);
        expect(canDraft.value).toBe(true);
        expect(compactionRestriction.value).toContain('Sending is unavailable');
        Object.assign(stored, {
          compactionReason: reason,
          compactionStartedWhileStreaming: true,
          stopping: false,
          compactionReconciliationPending: false,
        });
      }
    }
    Object.assign(stored, {
      streaming: false,
      compacting: false,
      compactionReconciliationPending: true,
    });
    expect(compactionRestriction.value).toBe('');
    expect(canQueue.value).toBe(false);
    expect(canCompose.value).toBe(false);
    stored.compactionReconciliationPending = false;
    expect(canCompose.value).toBe(true);
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
