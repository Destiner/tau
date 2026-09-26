/*
 * Tau's store: the reactive workspace state, the session controllers and
 * ephemeral sessions they drive, and the pure selectors over them. Kept
 * separate from the composable so the store can be shared with the Pi
 * runtime layer without a cycle.
 */
import { computed, reactive } from 'vue';

import type { CommandOption } from '../lib/commands';
import { errorCopy, feedbackTitle } from '../lib/error-copy';
import { scopeModels } from '../lib/pi/model-scope';
import type { ModelOption, ThinkingLevel } from '../lib/pi/model-scope';
import {
  emptyQueue,
  queueHasWork,
  visibleQueue,
  type QueueSnapshot,
  type QueueSubmission,
} from '../lib/pi/queue';
import type {
  HistoryLayer,
  LocalError,
  TranscriptEntry,
} from '../lib/pi/transcript';
import { asRecord, stringValue } from '../lib/pi/transcript';
import { recordControllerTransition } from '../lib/telemetry';
import type {
  ControllerLifecycleCause,
  ControllerLifecycleState,
  DraftLengthBucket,
} from '../lib/telemetry/attributes';
import type { TraceContext } from '../lib/telemetry/trace-context';

interface WorkspaceSnapshot {
  activeProjectPath: string;
  piPath: string | null;
  projects: ProjectSummary[];
}

interface ProjectSummary {
  path: string;
  name: string;
  workingDirectory: string;
  connectionString?: string;
  collapsed: boolean;
  selected: boolean;
  sessions: SessionSummary[];
}

interface SessionSummary {
  id: string;
  path: string;
  title: string;
  /** Bounded display-only source, before compact title normalization. */
  titleMarkdown?: string;
  /** The model id Pi last recorded for the session, empty when unknown. */
  model?: string;
  lastActive: string;
  lastUserMessageAt: number;
  /** Latest user message, falling back to the first agent message. */
  sortAt: number;
  archived: boolean;
  selected: boolean;
}

interface RemoteDirectoryEntry {
  name: string;
  path: string;
}

interface RemoteDirectoryListing {
  connectionString: string;
  workingDirectory: string;
  host: string;
  directories: RemoteDirectoryEntry[];
}

type ExtensionDialogMethod = 'select' | 'confirm' | 'input' | 'editor';

interface ExtensionDialog {
  key: string;
  requestId: string;
  method: ExtensionDialogMethod;
  title: string;
  message?: string;
  options?: string[];
  placeholder?: string;
  prefill?: string;
  draft: string;
  submitting: boolean;
  error: string;
  timeout?: number;
  controllerKey: string;
  runtimeId: string;
  generation: number;
  projectName: string;
  sessionName: string;
  /** Base for the file paths in the text; absent when the project is remote. */
  workingDirectory?: string;
}

const effortLabels: Record<ThinkingLevel, string> = {
  off: 'Off',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra High',
  max: 'Max',
};

type SessionIndicator = 'new' | 'draft' | 'working' | '';

const indicatorLabels: Record<Exclude<SessionIndicator, ''>, string> = {
  new: 'Unread',
  draft: 'Unsent draft',
  working: 'Working',
};

interface EphemeralSession extends SessionSummary {
  projectPath: string;
  controllerKey: string;
  createdAt: number;
  phantom: boolean;
}

interface PendingSessionRename {
  requestId: string;
  previousName: string;
  previousTitle: string;
  previousTitleMarkdown?: string;
}

interface SubmittedPrompt {
  requestId: string;
  generation: number;
  message: string;
  draft: string;
  /** Pi accepted prompt preflight, but may not have started or recorded a run. */
  accepted: boolean;
  /** Correlated post-preflight probes that bound ordinary prompt admission. */
  admissionStateRequestId?: string;
  admissionMessagesRequestId?: string;
  optimisticId?: string;
}

interface PendingPrompt {
  message: string;
  draft: string;
  optimisticId: string;
  command: boolean;
  stateRequestId: string;
  messagesRequestId: string;
  selectedModelProvider: string;
  selectedModelId: string;
  selectedModelName: string;
  selectedEffort: ThinkingLevel;
  settingsRequestId: string;
  settingsStep: '' | 'model' | 'effort';
  /** The `message.send` action span this prompt started under, if any, so
   * every RPC the pending-prompt flow later makes (across the `get_state`
   * round trip) still nests under the action that requested it. */
  telemetryContext?: TraceContext;
}

interface RetryPresentation {
  generation: number;
  attempt: number;
  reason: string;
}

type FeedbackAction = 'initialize' | 'reconnect' | 'reload';

interface FeedbackIncident {
  id: number;
  title: string;
  message: string;
  acknowledged: boolean;
  action?: FeedbackAction;
  controllerKey?: string;
}

interface SessionController {
  key: string;
  runtimeId: string;
  projectPath: string;
  sessionId: string;
  sessionPath: string;
  sessionName: string;
  phantom: boolean;
  generation: number;
  ready: boolean;
  streaming: boolean;
  compacting: boolean;
  /** A successful compaction is waiting for an authoritative message rebuild. */
  compactionReconciliationPending: boolean;
  /** First stream row id reserved for output emitted after that compaction. */
  compactionStreamSequence: number;
  /** Orders message hydrations within one Pi runtime generation and identity. */
  messagesHydrationSequence: number;
  stopping: boolean;
  starting: boolean;
  working: boolean;
  promptSubmitting: boolean;
  unread: boolean;
  lastUserMessageAt: number;
  /** Pi has reported visible user or assistant transcript activity. */
  hasPiTranscript: boolean;
  /** A post-message_end RPC barrier or settled hydration proved persistence. */
  materializationVerified: boolean;
  /** Correlates the ordering barrier sent after the first assistant message_end. */
  materializationBarrierRequestId: string;
  /** The current run settled, so its following hydrations may verify storage. */
  postSettlementHydration: boolean;
  /** The settled UI transcript had meaningful assistant-side activity. */
  settledAssistantActivity: boolean;
  /** Correlates the settled state read whose hydration may verify storage. */
  materializationStateRequestId: string;
  /** Correlates the hydration currently allowed to verify materialization. */
  materializationMessagesRequestId: string;
  messages: TranscriptEntry[];
  /** Whether `get_messages` has answered once, so an empty transcript can be
   * read as a session without history rather than one that has yet to load. */
  messagesLoaded: boolean;
  /** Raw-history layers fetched on demand from Pi's append-only entry tree. */
  historyLayers: HistoryLayer[];
  /** First raw-history layer currently rendered above the compacted tail. */
  firstVisibleHistoryLayer: number;
  /** Number of rendered rows that belong to the raw-history prefix. */
  historyPrefixLength: number;
  /** Correlates the one full-history request this controller may have in flight. */
  historyRequestId: string;
  /** Failures Pi reports as events only; its message list never carries them. */
  localErrors: LocalError[];
  draft: string;
  retry?: RetryPresentation;
  feedback: FeedbackIncident[];
  currentModelProvider: string;
  currentModelId: string;
  currentModelName: string;
  currentEffort: ThinkingLevel;
  pendingEffort: ThinkingLevel | '';
  pendingSettingRequestId: string;
  models: ModelOption[];
  modelScope: string[];
  efforts: ThinkingLevel[];
  commands: CommandOption[];
  commandsLoaded: boolean;
  pendingPrompt?: PendingPrompt;
  submittedPrompt?: SubmittedPrompt;
  queue: QueueSnapshot;
  queueVersion: number;
  queueSubmissions: QueueSubmission[];
  queueSteeringMode: string;
  queueFollowUpMode: string;
  queuePreparing: boolean;
  queueClearing: boolean;
  queueFeedback: string;
  queueFailedDrafts: string[];
  pendingSessionRename?: PendingSessionRename;
  /** Advances whenever a name notification or optimistic rename invalidates older reads. */
  sessionNameRevision: number;
  /** Correlates the identity-bearing read scheduled by a name notification. */
  sessionNameStateRequestId: string;
  sessionNameStateRevision: number;
  bootstrapStateRequestId: string;
  bootstrapSessionPath: string;
  runStateRequestId: string;
  startMessagesRequestId: string;
  commandPromptRequestId: string;
  commandSyncRequestId: string;
  replacementProbeRequestId: string;
  abortProbeRequestId: string;
  connectingRemote: boolean;
  /** An established remote runtime exited and requires an explicit reconnect. */
  remoteDisconnected: boolean;
  /** The explicit reconnect action currently owns this bootstrap attempt. */
  reconnectingRemote: boolean;
  remoteConnectionTimedOut: boolean;
  syncing: boolean;
  lastActiveSequence: number;
  disposed: boolean;
  streamSequence: number;
}

/** The subset of a controller's boolean flags a lifecycle transition is
 * derived from. Not every boolean on `SessionController` is lifecycle-
 * critical (`unread`, `phantom`, `disposed`, and the various request-id
 * strings are bookkeeping, not run state), and `disposed` is a one-way
 * terminal flag set immediately before a controller is spliced out of
 * `state.controllers`, so it is deliberately excluded from the derived
 * state a transition compares. */
type ControllerLifecycleField =
  | 'ready'
  | 'streaming'
  | 'stopping'
  | 'starting'
  | 'working'
  | 'syncing'
  | 'connectingRemote';

type ControllerLifecyclePatch = Partial<
  Pick<SessionController, ControllerLifecycleField>
>;

/**
 * Derives one coarse, named lifecycle state from a controller's boolean
 * flags, in priority order (most specific/blocking first). This is the
 * "state" `setControllerLifecycle` compares before and after a mutation:
 * not every boolean toggle changes it (`working` flipping while `streaming`
 * is already true never does), but a move between these seven names always
 * does, and that is exactly the kind of change worth a persisted record.
 */
function classifyControllerLifecycle(
  controller: SessionController,
): ControllerLifecycleState {
  if (controller.connectingRemote) return 'connecting';
  if (controller.starting) return 'starting';
  if (controller.stopping) return 'stopping';
  if (controller.syncing) return 'syncing';
  if (controller.working || controller.streaming) return 'working';
  if (controller.ready) return 'ready';
  return 'idle';
}

/**
 * Applies `patch` to `controller`'s lifecycle-critical fields and, if doing
 * so changes the derived composite state, records one `controller.lifecycle`
 * transition naming `cause` and carrying `context` when the mutation
 * happened inside an active span (a semantic action or an RPC response).
 *
 * This replaces every lifecycle-critical direct assignment in
 * `src/lib/pi/runtime.ts` and `src/composables/useTau.ts`: calling it here
 * instead of assigning the fields directly is what makes a missing expected
 * transition visible as an absence in the persisted timeline (a response or
 * action is recorded, but no matching transition follows) rather than a
 * change nothing has any evidence for either way.
 */
function setControllerLifecycle(
  controller: SessionController,
  patch: ControllerLifecyclePatch,
  cause: ControllerLifecycleCause,
  context?: TraceContext,
): void {
  const before = classifyControllerLifecycle(controller);
  Object.assign(controller, patch);
  const after = classifyControllerLifecycle(controller);
  if (before === after) return;
  recordControllerTransition(
    before,
    after,
    cause,
    {
      sessionId: controller.sessionId,
      controllerId: controller.key,
      runtimeId: controller.runtimeId,
      generation: controller.generation || undefined,
    },
    context,
  );
}

interface RemoteRetry {
  controllerKey: string;
  projectPath: string;
  sessionPath?: string;
  preserveMessages: boolean;
}

type RemoteDialogMode = 'add' | 'retry';
type RemoteDialogStep = 'connection' | 'directory';
type RemoteDirectoryChoice = 'back' | 'select' | 'forward';

const state = reactive({
  workspace: null as WorkspaceSnapshot | null,
  initializing: false,
  ownershipFailure: null as 'conflict' | 'retryable' | null,
  activeProjectPath: '',
  activeSessionId: '',
  activeSessionPath: '',
  activeControllerKey: '',
  workspaceFeedback: [] as FeedbackIncident[],
  controllers: [] as SessionController[],
  ephemeralSessions: [] as EphemeralSession[],
  extensionDialogs: [] as ExtensionDialog[],
  remoteDialogOpen: false,
  remoteDialogMode: 'add' as RemoteDialogMode,
  remoteDialogStep: 'connection' as RemoteDialogStep,
  remoteConnectionString: '',
  remoteConnectionError: '',
  remoteConnecting: false,
  remoteDirectoryHost: '',
  remoteDirectoryRoot: '',
  remoteWorkingDirectory: '',
  remoteDirectoryHistory: [] as string[],
  remoteDirectories: [] as RemoteDirectoryEntry[],
  remoteDirectoryFilter: '',
  remoteDirectorySelectedIndex: 0,
  remoteRetry: undefined as RemoteRetry | undefined,
  removingProjectPaths: [] as string[],
  requestSequence: 0,
});

function sessionWorkInProgress(controller: SessionController): boolean {
  if (controller.disposed) return false;
  const lifecycle = classifyControllerLifecycle(controller);
  return (
    (lifecycle !== 'idle' && lifecycle !== 'ready') ||
    controller.compacting ||
    controller.compactionReconciliationPending ||
    controller.promptSubmitting ||
    Boolean(controller.pendingPrompt) ||
    Boolean(controller.submittedPrompt) ||
    controller.queuePreparing ||
    controller.queueClearing ||
    controller.queueFailedDrafts.length > 0 ||
    queueHasWork(controller.queue, controller.queueSubmissions) ||
    controllerHasPendingDialog(controller)
  );
}

const inProgressSessionCount = computed(
  () =>
    new Set(
      state.controllers
        .filter(sessionWorkInProgress)
        .map((controller) =>
          controller.sessionId
            ? `${controller.projectPath}\u0000${controller.sessionId}`
            : controller.key,
        ),
    ).size,
);

/**
 * Pi has no session-replacement event, and a `get_state` sent the moment a run
 * settles still reports the outgoing session because extensions swap sessions
 * from a deferred callback. These delays re-check session identity after the
 * moments a replacement can happen, so a spawned phase session is registered
 * without waiting for its first user message.
 */
const replacementProbeDelays = [150, 450, 1_000, 2_000, 3_200];

/**
 * Pi answers an abort only once the agent has gone idle, so a tool call that
 * ignores the abort signal withholds both the acknowledgement and the settle
 * event for as long as it keeps running. Tau re-reads Pi's own run state after
 * this grace period rather than leaving the session locked on a stop it cannot
 * confirm.
 */
const abortAcknowledgeDelay = 2_000;

/**
 * Stopping an idle runtime is not the neutral act it looks like. Pi hands a
 * session-opening context to extensions only inside a command handler or a
 * `withSession` callback, so an extension that drives a multi-session workflow
 * has to hold that context in the process it was given. The process is the
 * only place that state exists: reopening the session file restores the
 * transcript but not the handoff, and a workflow phase that spans a user turn
 * then completes with nothing left to open its next session.
 *
 * Idle hidden runtimes are therefore kept warm, and only the least recently
 * active ones past this limit are released. Runtimes that are still working
 * are never released, so the live total can exceed the limit while work runs.
 */
const idleRuntimeLimit = 6;

let phantomSequence = 0;
let controllerSequence = 0;
let activitySequence = 0;
let feedbackSequence = 0;
const extensionDialogTimeouts = new Map<
  string,
  ReturnType<typeof setTimeout>
>();
const replacementProbeTimers = new Map<
  string,
  ReturnType<typeof setTimeout>[]
>();
const abortProbeTimers = new Map<string, ReturnType<typeof setTimeout>>();

const emptyMessages: TranscriptEntry[] = [];
const emptyModels: ModelOption[] = [];
const emptyEfforts: ThinkingLevel[] = [];
const emptyCommands: CommandOption[] = [];

const activeProject = computed(() =>
  state.workspace?.projects.find(
    (project) => project.path === state.activeProjectPath,
  ),
);

const projectActionsDisabled = computed(
  () =>
    state.initializing ||
    state.ownershipFailure !== null ||
    state.removingProjectPaths.length > 0,
);

// An archived session is browsable: look past the per-project list, which
// excludes archived rows. Ephemeral sessions are not workspace rows at all,
// so they are searched first.
const activeSession = computed(() => {
  const project = activeProject.value;
  if (!project) return undefined;
  return (
    state.ephemeralSessions.find(
      (session) =>
        session.id === state.activeSessionId &&
        session.projectPath === project.path,
    ) ??
    project.sessions.find((session) => session.id === state.activeSessionId)
  );
});

const activeController = computed(() =>
  state.controllers.find(
    (controller) => controller.key === state.activeControllerKey,
  ),
);

/** One flat row of the archived-sessions view: a session plus its project. */
interface ArchivedSessionEntry {
  projectPath: string;
  projectName: string;
  session: SessionSummary;
}

/**
 * Archived sessions across every project, newest first. The view is not
 * per-project by design: archive review is workspace-wide.
 */
const archivedSessionEntries = computed<ArchivedSessionEntry[]>(() => {
  const entries: ArchivedSessionEntry[] = [];
  for (const project of state.workspace?.projects ?? []) {
    for (const session of project.sessions) {
      if (!session.archived) continue;
      entries.push({
        projectPath: project.path,
        projectName: project.name,
        session,
      });
    }
  }
  return entries.sort(
    (left, right) =>
      right.session.sortAt - left.session.sortAt ||
      right.session.lastUserMessageAt - left.session.lastUserMessageAt,
  );
});

const messages = computed(
  () => activeController.value?.messages ?? emptyMessages,
);
const draft = computed({
  get: () => activeController.value?.draft ?? '',
  set: (value: string) => {
    const controller = activeController.value;
    if (!controller) return;
    controller.draft = value;
    const session = ephemeralSessionByController(controller.key);
    if (session?.phantom) {
      session.title = draftTitle(value);
      session.titleMarkdown = sessionTitleMarkdown(value);
    }
  },
});
const retryPresentation = computed(() => activeController.value?.retry);
const activeFeedback = computed(() => {
  const controllerFeedback = activeController.value?.feedback.find(
    (incident) => !incident.acknowledged,
  );
  return (
    controllerFeedback ??
    state.workspaceFeedback.find((incident) => !incident.acknowledged)
  );
});
const streaming = computed(() => activeController.value?.streaming === true);
const activeQueue = computed(() => {
  const controller = activeController.value;
  return controller
    ? visibleQueue(controller.queue, controller.queueSubmissions)
    : emptyQueue();
});
const queueFeedback = computed(
  () => activeController.value?.queueFeedback ?? '',
);
const queueBusy = computed(() =>
  Boolean(
    activeController.value?.queueClearing ||
    activeController.value?.queuePreparing ||
    activeController.value?.queueSubmissions.length,
  ),
);
const queueFailedDrafts = computed(
  () => activeController.value?.queueFailedDrafts ?? [],
);
const canQueue = computed(() => {
  const controller = activeController.value;
  return Boolean(
    controller &&
    canDraft.value &&
    controller.ready &&
    !controller.phantom &&
    controller.streaming &&
    !controller.stopping &&
    !controller.compacting &&
    !controller.compactionReconciliationPending &&
    !controller.starting &&
    !controller.pendingPrompt &&
    !controller.submittedPrompt &&
    !controller.queueClearing &&
    !controller.remoteDisconnected,
  );
});
const compacting = computed(() => activeController.value?.compacting === true);
const stopping = computed(() => activeController.value?.stopping === true);
const promptSubmitting = computed(
  () => activeController.value?.promptSubmitting === true,
);
const models = computed(() => {
  const controller = activeController.value;
  if (!controller) return emptyModels;
  return scopeModels(controller.models, controller.modelScope);
});
const efforts = computed(() => activeController.value?.efforts ?? emptyEfforts);
const commands = computed(
  () => activeController.value?.commands ?? emptyCommands,
);
const activeExtensionDialog = computed(() => {
  const controller = activeController.value;
  if (!controller) return undefined;
  return state.extensionDialogs.find(
    (dialog) => dialog.controllerKey === controller.key,
  );
});
const currentModelProvider = computed(
  () => activeController.value?.currentModelProvider ?? '',
);
const currentModelId = computed(
  () => activeController.value?.currentModelId ?? '',
);
const currentEffort = computed(
  () => activeController.value?.currentEffort ?? 'off',
);

const canDraft = computed(() =>
  Boolean(
    activeProject.value &&
    activeSession.value &&
    activeController.value &&
    !projectActionsDisabled.value,
  ),
);

/**
 * A saved session has nothing to show until its runtime hydrates the
 * transcript, so the pane reports loading instead of rendering the empty
 * session composer over a session that already holds messages.
 */
const sessionLoading = computed(() => {
  const controller = activeController.value;
  if (!controller || controller.phantom) return false;
  if (
    controller.messages.length > 0 ||
    controller.feedback.some((incident) => !incident.acknowledged)
  )
    return false;
  return !controller.ready || controller.starting || controller.syncing;
});

const canReconnectRemote = computed(() => {
  const controller = activeController.value;
  return Boolean(
    activeProject.value?.connectionString &&
    controller?.remoteDisconnected &&
    !controller.reconnectingRemote &&
    !controller.starting &&
    !controller.disposed &&
    !projectActionsDisabled.value,
  );
});

const canCompose = computed(() => {
  const controller = activeController.value;
  if (!activeProject.value || !activeSession.value || !controller) return false;
  const admittingOrdinaryPrompt = Boolean(
    controller.submittedPrompt?.optimisticId,
  );
  if (
    controller.starting ||
    controller.working ||
    controller.promptSubmitting ||
    admittingOrdinaryPrompt ||
    projectActionsDisabled.value
  )
    return false;
  return controller.phantom
    ? runtimeAvailable(activeProject.value)
    : controller.ready;
});

const sessionTitle = computed(() => {
  const controller = activeController.value;
  return (
    controller?.sessionName ||
    activeSession.value?.title ||
    firstUserMessage(controller) ||
    'New Session'
  );
});

const currentModelLabel = computed(() => {
  const controller = activeController.value;
  return controller?.currentModelName || controller?.currentModelId || 'Model';
});

const currentEffortLabel = computed(
  () => effortLabels[activeController.value?.currentEffort ?? 'off'],
);

const settingsDisabled = computed(() => {
  const controller = activeController.value;
  return (
    !controller ||
    (!controller.ready && !controller.phantom) ||
    controller.streaming ||
    controller.stopping ||
    controller.starting ||
    Boolean(controller.pendingSettingRequestId) ||
    projectActionsDisabled.value
  );
});

/**
 * Pi keeps the name inside the session it is running, so renaming needs a live
 * runtime and a session Pi has already opened. An unsent session shows a
 * preview of its draft instead of a name, so it has nothing to rename yet.
 */
const canRenameSession = computed(() => {
  const controller = activeController.value;
  return Boolean(
    controller &&
    !controller.phantom &&
    controller.ready &&
    !controller.pendingSessionRename &&
    !projectActionsDisabled.value,
  );
});

function isProjectRemoving(projectPath: string): boolean {
  return state.removingProjectPaths.includes(projectPath);
}

function canArchiveSession(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  if (
    projectActionsDisabled.value ||
    ephemeralSession(project.path, session.id) !== undefined
  ) {
    return false;
  }
  const controller = controllerForSession(project.path, session.id);
  if (!controller) return true;
  const lifecycle = classifyControllerLifecycle(controller);
  return lifecycle === 'idle' || lifecycle === 'ready';
}

function projectSessions(project: ProjectSummary): SessionSummary[] {
  const ephemeral = state.ephemeralSessions.filter(
    (session) => session.projectPath === project.path,
  );
  const ephemeralIds = new Set(ephemeral.map((session) => session.id));
  const phantomCreatedAt = new Map(
    ephemeral
      .filter((session) => session.phantom)
      .map((session) => [session.id, session.createdAt]),
  );
  return [
    ...ephemeral,
    ...project.sessions.filter(
      (session) => !session.archived && !ephemeralIds.has(session.id),
    ),
  ].sort((left, right) => {
    const leftPhantomCreatedAt = phantomCreatedAt.get(left.id);
    const rightPhantomCreatedAt = phantomCreatedAt.get(right.id);
    if (
      leftPhantomCreatedAt !== undefined ||
      rightPhantomCreatedAt !== undefined
    ) {
      if (leftPhantomCreatedAt === undefined) return 1;
      if (rightPhantomCreatedAt === undefined) return -1;
      return rightPhantomCreatedAt - leftPhantomCreatedAt;
    }
    return (
      sessionSortAt(project.path, right) - sessionSortAt(project.path, left)
    );
  });
}

function sessionSortAt(projectPath: string, session: SessionSummary): number {
  return Math.max(
    session.sortAt,
    session.lastUserMessageAt,
    controllerForSession(projectPath, session.id)?.lastUserMessageAt ?? 0,
  );
}

function sessionLastActive(
  project: ProjectSummary,
  session: SessionSummary,
): string {
  const controller = controllerForSession(project.path, session.id);
  if (
    ephemeralSession(project.path, session.id) &&
    !controller?.messages.some(
      (message) => message.kind === 'user' || message.kind === 'assistant',
    )
  ) {
    return '';
  }
  const timestamp = sessionLastUserMessageAt(project.path, session);
  return timestamp > session.lastUserMessageAt
    ? relativeTimestamp(timestamp)
    : session.lastActive;
}

function sessionLastUserMessageAt(
  projectPath: string,
  session: SessionSummary,
): number {
  return Math.max(
    session.lastUserMessageAt,
    controllerForSession(projectPath, session.id)?.lastUserMessageAt ?? 0,
  );
}

function isSessionSelected(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return (
    project.path === state.activeProjectPath &&
    session.id === state.activeSessionId
  );
}

function sessionIndicator(
  project: ProjectSummary,
  session: SessionSummary,
): SessionIndicator {
  const controller = controllerForSession(project.path, session.id);
  if (!controller) return '';
  if (controllerHasPendingDialog(controller)) return 'new';
  if (controller.working) return 'working';
  if (controller.draft.trim()) return 'draft';
  return controller.unread ? 'new' : '';
}

function isSessionUnread(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return controllerForSession(project.path, session.id)?.unread === true;
}

function markSessionUnread(
  project: ProjectSummary,
  session: SessionSummary,
): void {
  ensureController(project, session).unread = true;
}

function markSessionRead(
  project: ProjectSummary,
  session: SessionSummary,
): void {
  const controller = controllerForSession(project.path, session.id);
  if (controller) controller.unread = false;
}

function indicatorLabel(indicator: SessionIndicator): string {
  return indicator ? indicatorLabels[indicator] : '';
}

function sessionTooltipStatus(
  project: ProjectSummary,
  session: SessionSummary,
): string {
  if (session.archived) return 'Archived';
  return indicatorLabel(sessionIndicator(project, session)) || 'Idle';
}

function expandedRelativeTime(value: string): string {
  if (value === 'now') return 'just now';
  const match = /^(\d+)([mhdwy])$/.exec(value);
  if (!match) return value;
  const count = Number(match[1]);
  const unit = { m: 'minute', h: 'hour', d: 'day', w: 'week', y: 'year' }[
    match[2] as 'm' | 'h' | 'd' | 'w' | 'y'
  ];
  return `${count} ${unit}${count === 1 ? '' : 's'} ago`;
}

function projectIndicator(project: ProjectSummary): SessionIndicator {
  if (!project.collapsed) return '';
  const indicators = new Set(
    projectSessions(project).map((session) =>
      sessionIndicator(project, session),
    ),
  );
  for (const indicator of ['new', 'working', 'draft'] as const) {
    if (indicators.has(indicator)) return indicator;
  }
  return '';
}

function setActiveSessionView(
  project: ProjectSummary,
  session: SessionSummary,
  controller: SessionController,
): void {
  state.activeProjectPath = project.path;
  state.activeSessionId = session.id;
  state.activeSessionPath = session.path;
  state.activeControllerKey = controller.key;
  controller.unread = false;
  touchController(controller);
}

function createPhantomSession(
  projectPath: string,
  controllerKey: string,
): EphemeralSession {
  phantomSequence += 1;
  const now = Date.now();
  return {
    id: `phantom-${now}-${phantomSequence}`,
    path: '',
    title: 'New Session',
    titleMarkdown: 'New Session',
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: now,
    archived: false,
    selected: true,
    projectPath,
    controllerKey,
    createdAt: now * 1000 + phantomSequence,
    phantom: true,
  };
}

// An archived record is not a row the user can reach, so a controller backed
// by one is not represented in the workspace: its ephemeral row is the only
// handle on the session and has to stay.
function workspaceContainsSession(controller: SessionController): boolean {
  return (
    state.workspace?.projects
      .find((project) => project.path === controller.projectPath)
      ?.sessions.some((session) => session.id === controller.sessionId) === true
  );
}

const interruptedQueueDrafts = new Map<
  string,
  { drafts: string[]; lostQueuedMessages: boolean }
>();

function queueRecoveryKey(projectPath: string, sessionId: string): string {
  return `${projectPath}\u0000${sessionId}`;
}

function stashInterruptedQueueDrafts(
  controller: SessionController,
  drafts: string[],
  lostQueuedMessages = false,
): void {
  if ((!drafts.length && !lostQueuedMessages) || !controller.sessionId) return;
  const key = queueRecoveryKey(controller.projectPath, controller.sessionId);
  const previous = interruptedQueueDrafts.get(key);
  interruptedQueueDrafts.set(key, {
    drafts: [...(previous?.drafts ?? []), ...drafts],
    lostQueuedMessages: Boolean(
      previous?.lostQueuedMessages || lostQueuedMessages,
    ),
  });
}

function ensureController(
  project: ProjectSummary,
  session: SessionSummary,
): SessionController {
  const controller =
    controllerForSession(project.path, session.id) ??
    createController(project, session, nextControllerKey());
  const key = queueRecoveryKey(project.path, session.id);
  const recovered = interruptedQueueDrafts.get(key);
  if (recovered) {
    controller.queueFailedDrafts.push(...recovered.drafts);
    controller.queueFeedback = recovered.lostQueuedMessages
      ? 'Pending messages were lost when Pi switched sessions. They were not resent.'
      : 'Review unsent messages before retrying.';
    interruptedQueueDrafts.delete(key);
  }
  return controller;
}

function inheritControllerSettings(
  controller: SessionController,
  preferred: SessionController | undefined,
): void {
  const source =
    preferred &&
    (preferred.models.length > 0 ||
      preferred.efforts.length > 0 ||
      preferred.currentModelId)
      ? preferred
      : [...state.controllers]
          .reverse()
          .find(
            (candidate) =>
              candidate.key !== controller.key &&
              (candidate.models.length > 0 ||
                candidate.efforts.length > 0 ||
                candidate.currentModelId),
          );
  if (source) {
    controller.models = [...source.models];
    // The scope belongs to the catalogue it narrows. A session that inherits
    // one without the other never starts a runtime to read the scope back, and
    // would offer every model Pi knows.
    controller.modelScope = [...source.modelScope];
    controller.efforts = [...source.efforts];
    controller.currentModelProvider = source.currentModelProvider;
    controller.currentModelId = source.currentModelId;
    controller.currentModelName = source.currentModelName;
    controller.currentEffort = source.currentEffort;
  }

  const commandSource = [...state.controllers]
    .reverse()
    .find(
      (candidate) =>
        candidate.key !== controller.key &&
        candidate.projectPath === controller.projectPath &&
        candidate.commandsLoaded,
    );
  if (commandSource) {
    controller.commands = [...commandSource.commands];
    controller.commandsLoaded = true;
  }
}

function createController(
  project: ProjectSummary,
  session: SessionSummary,
  key: string,
): SessionController {
  const phantom =
    ('phantom' in session && session.phantom === true) ||
    ephemeralSession(project.path, session.id)?.phantom === true;
  const controller: SessionController = reactive({
    key,
    runtimeId: `runtime-${key}`,
    projectPath: project.path,
    sessionId: session.id,
    sessionPath: session.path,
    sessionName: session.title === 'New Session' ? '' : session.title,
    phantom,
    generation: 0,
    ready: false,
    streaming: false,
    compacting: false,
    compactionReconciliationPending: false,
    compactionStreamSequence: 0,
    messagesHydrationSequence: 0,
    stopping: false,
    starting: false,
    working: false,
    promptSubmitting: false,
    unread: false,
    lastUserMessageAt: session.lastUserMessageAt,
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
    retry: undefined,
    feedback: [],
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
    submittedPrompt: undefined,
    queue: emptyQueue(),
    queueVersion: 0,
    queueSubmissions: [],
    queueSteeringMode: '',
    queueFollowUpMode: '',
    queuePreparing: false,
    queueClearing: false,
    queueFeedback: '',
    queueFailedDrafts: [],
    pendingSessionRename: undefined,
    sessionNameRevision: 0,
    sessionNameStateRequestId: '',
    sessionNameStateRevision: 0,
    bootstrapStateRequestId: '',
    bootstrapSessionPath: '',
    runStateRequestId: '',
    startMessagesRequestId: '',
    commandPromptRequestId: '',
    commandSyncRequestId: '',
    replacementProbeRequestId: '',
    abortProbeRequestId: '',
    connectingRemote: false,
    remoteDisconnected: false,
    reconnectingRemote: false,
    remoteConnectionTimedOut: false,
    syncing: false,
    lastActiveSequence: (activitySequence += 1),
    disposed: false,
    streamSequence: 0,
  });
  state.controllers.push(controller);
  return controller;
}

function controllerForSession(
  projectPath: string,
  sessionId: string,
): SessionController | undefined {
  const ephemeral = ephemeralSession(projectPath, sessionId);
  if (ephemeral) return controllerByKey(ephemeral.controllerKey);
  return state.controllers.find(
    (controller) =>
      controller.projectPath === projectPath &&
      controller.sessionId === sessionId,
  );
}

function controllerByKey(key: string): SessionController | undefined {
  return state.controllers.find((controller) => controller.key === key);
}

function controllerByRuntimeId(
  runtimeId: string,
): SessionController | undefined {
  return state.controllers.find(
    (controller) => controller.runtimeId === runtimeId,
  );
}

function ephemeralSession(
  projectPath: string,
  sessionId: string,
): EphemeralSession | undefined {
  return state.ephemeralSessions.find(
    (session) =>
      session.projectPath === projectPath && session.id === sessionId,
  );
}

function ephemeralSessionByController(
  controllerKey: string,
): EphemeralSession | undefined {
  return state.ephemeralSessions.find(
    (session) => session.controllerKey === controllerKey,
  );
}

function isControllerSelected(controller: SessionController): boolean {
  return controller.key === state.activeControllerKey;
}

function controllerHasPendingDialog(controller: SessionController): boolean {
  return state.extensionDialogs.some(
    (dialog) => dialog.controllerKey === controller.key,
  );
}

function runtimeAvailable(project: ProjectSummary): boolean {
  if (project.connectionString) return true;
  return Boolean(state.workspace?.piPath);
}

function touchController(controller: SessionController): void {
  activitySequence += 1;
  controller.lastActiveSequence = activitySequence;
}

function markUserMessageSubmitted(controller: SessionController): void {
  controller.lastUserMessageAt = Date.now();
  const session = ephemeralSessionByController(controller.key);
  if (session) {
    session.lastUserMessageAt = controller.lastUserMessageAt;
    session.lastActive = 'now';
  }
}

function relativeTimestamp(timestamp: number): string {
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1_000));
  if (seconds < 60) return 'now';
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`;
  if (seconds < 604_800) return `${Math.floor(seconds / 86_400)}d`;
  if (seconds < 31_536_000) return `${Math.floor(seconds / 604_800)}w`;
  return `${Math.floor(seconds / 31_536_000)}y`;
}

function draftTitle(value: string): string {
  return normalizeSessionName(value) || 'New Session';
}

function sessionTitleMarkdown(value: string): string {
  return value.trim()
    ? Array.from(value).slice(0, 240).join('')
    : 'New Session';
}

function tooltipTitleMarkdown(session: SessionSummary): string {
  return sessionTitleMarkdown(session.titleMarkdown ?? session.title);
}

function normalizeSessionName(value: string): string {
  return value.replace(/\s+/g, ' ').trim().slice(0, 240);
}

function commandOption(value: unknown): CommandOption | undefined {
  const command = asRecord(value);
  const name = stringValue(command?.name);
  const source = stringValue(command?.source);
  if (
    !name ||
    (source !== 'extension' && source !== 'prompt' && source !== 'skill')
  ) {
    return undefined;
  }

  const description = stringValue(command?.description);
  return {
    name,
    ...(description ? { description } : {}),
    source,
  };
}

function normalizeEffort(value: unknown): ThinkingLevel {
  return value === 'minimal' ||
    value === 'low' ||
    value === 'medium' ||
    value === 'high' ||
    value === 'xhigh' ||
    value === 'max'
    ? value
    : 'off';
}

function nextRequestId(label: string): string {
  state.requestSequence += 1;
  return `tau-${label}-${state.requestSequence}`;
}

function nextControllerKey(): string {
  controllerSequence += 1;
  return `${Date.now()}-${controllerSequence}`;
}

function firstUserMessage(controller: SessionController | undefined): string {
  const entry = controller?.messages.find(
    (message) => message.kind === 'user' || message.kind === 'skill',
  );
  return entry?.kind === 'skill'
    ? `/skill:${entry.skillName || 'skill'}`
    : (entry?.text ?? '');
}

function clearActiveSession(): void {
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.activeControllerKey = '';
}

function clearRemoteDirectoryBrowser(): void {
  state.remoteDirectoryHost = '';
  state.remoteDirectoryRoot = '';
  state.remoteWorkingDirectory = '';
  state.remoteDirectoryHistory = [];
  state.remoteDirectories = [];
  state.remoteDirectoryFilter = '';
  state.remoteDirectorySelectedIndex = 0;
}

function applyRemoteDirectoryListing(
  listing: RemoteDirectoryListing,
  initial = false,
): void {
  state.remoteConnectionString = listing.connectionString;
  state.remoteDirectoryHost = listing.host;
  if (initial) {
    state.remoteDirectoryRoot = listing.workingDirectory;
    state.remoteDirectoryHistory = [];
  }
  state.remoteWorkingDirectory = listing.workingDirectory;
  state.remoteDirectories = listing.directories;
  state.remoteDirectoryFilter = '';
  state.remoteDirectorySelectedIndex = 0;
  state.remoteConnectionError = '';
}

function presentRemoteConnectionError(
  controller: SessionController,
  message: string,
): void {
  setControllerLifecycle(
    controller,
    {
      ready: false,
      streaming: false,
      stopping: false,
      starting: false,
      working: false,
      connectingRemote: false,
      syncing: false,
    },
    'bridge_event_failed',
  );
  controller.runStateRequestId = '';
  if (!isControllerSelected(controller)) return;

  const project = state.workspace?.projects.find(
    (item) => item.path === controller.projectPath,
  );
  if (!project?.connectionString) return;
  state.remoteRetry = {
    controllerKey: controller.key,
    projectPath: project.path,
    sessionPath: controller.sessionPath || undefined,
    preserveMessages: controller.messages.length > 0,
  };
  state.remoteDialogMode = 'retry';
  state.remoteDialogStep = 'connection';
  state.remoteConnectionString = project.connectionString;
  clearRemoteDirectoryBrowser();
  state.remoteConnectionError = message;
  state.remoteConnecting = false;
  state.remoteDialogOpen = true;
}

function clearRemoteRetry(): void {
  state.remoteRetry = undefined;
}

function finishRemoteConnection(controller: SessionController): void {
  if (state.remoteRetry?.controllerKey !== controller.key) return;
  state.remoteRetry = undefined;
  state.remoteConnecting = false;
  if (state.remoteDialogMode === 'retry') {
    state.remoteDialogOpen = false;
    state.remoteConnectionError = '';
    clearRemoteDirectoryBrowser();
  }
}

function feedbackAction(message: string): FeedbackAction | undefined {
  if (message === errorCopy.piOwnership) return 'initialize';
  if (
    message === errorCopy.piNotFound ||
    message === errorCopy.piOwnershipConflict ||
    message === errorCopy.loadWorkspace
  )
    return 'reload';
  if (
    message === 'The connection was lost.' ||
    message.startsWith('The remote connection')
  )
    return 'reconnect';
  return undefined;
}

function feedbackFamily(message: string): string {
  if (
    message === 'The connection was lost.' ||
    /\b(?:Pi|remote) (?:connection|process)\b|remote connection/i.test(message)
  )
    return 'runtime-connection';
  return message;
}

function publishFeedback(
  incidents: FeedbackIncident[],
  message: string,
  controllerKey?: string,
): FeedbackIncident {
  const family = feedbackFamily(message);
  const previous = [...incidents]
    .reverse()
    .find(
      (incident) =>
        !incident.acknowledged && feedbackFamily(incident.message) === family,
    );
  if (previous) {
    previous.title = feedbackTitle(message);
    previous.message = message;
    previous.action = feedbackAction(message);
    return previous;
  }
  const incident = {
    id: (feedbackSequence += 1),
    title: feedbackTitle(message),
    message,
    acknowledged: false,
    ...(feedbackAction(message) ? { action: feedbackAction(message) } : {}),
    ...(controllerKey ? { controllerKey } : {}),
  } satisfies FeedbackIncident;
  incidents.push(incident);
  if (incidents.length > 20) incidents.splice(0, incidents.length - 20);
  return incident;
}

function setControllerError(
  controller: SessionController,
  message: string,
): void {
  publishFeedback(controller.feedback, message, controller.key);
  if (!isControllerSelected(controller)) controller.unread = true;
}

function clearControllerFeedback(
  controller: SessionController | undefined,
  message?: string,
): void {
  if (!controller) return;
  for (const incident of controller.feedback) {
    if (!message || incident.message === message) incident.acknowledged = true;
  }
}

function setWorkspaceError(message: string): void {
  publishFeedback(state.workspaceFeedback, message);
}

function acknowledgeFeedback(incident: FeedbackIncident): void {
  incident.acknowledged = true;
}

function reopenRemoteFeedback(controller: SessionController): void {
  const incident = [...controller.feedback]
    .reverse()
    .find((candidate) => candidate.action === 'reconnect');
  if (incident) incident.acknowledged = false;
}

function resolveRemoteFeedback(controller: SessionController): void {
  for (const incident of controller.feedback) {
    if (incident.action === 'reconnect') incident.acknowledged = true;
  }
}

const MAX_STATE_SUMMARY_COUNT = 1_000_000;

interface TranscriptKindCounts {
  user: number;
  assistant: number;
  tool: number;
  thinking: number;
  error: number;
}

interface StateSnapshot {
  controllerCount: number;
  runtimeCount: number;
  activeControllerCount: number;
  notificationCount: number;
  dialogCount: number;
  transcriptCounts: TranscriptKindCounts;
  draftBucket: DraftLengthBucket;
  /** Active session/controller identifiers, when applicable — attached the
   * same way any other log's context is, never a new family-specific
   * attribute. No project identifier: Stage 3 already deferred one until an
   * installation-local project identifier exists. */
  scope: { sessionId?: string; controllerId?: string };
}

/** Buckets a draft's length, never its text: `frontend.state_summary` is
 * content-free by construction, so only the bucket ever leaves this
 * function. */
function draftLengthBucket(length: number): DraftLengthBucket {
  if (length === 0) return 'empty';
  if (length <= 50) return 'short';
  if (length <= 500) return 'medium';
  return 'long';
}

/**
 * Builds one periodic, content-free snapshot of workspace shape: counts and
 * a length bucket only, never transcript or draft text, paths, or names.
 * Read by `lib/telemetry/heartbeat`'s timer and otherwise unused — this is
 * the one place that walks every controller's message list for this
 * purpose, so the counting logic exists once.
 */
function buildStateSnapshot(): StateSnapshot {
  const transcriptCounts: TranscriptKindCounts = {
    user: 0,
    assistant: 0,
    tool: 0,
    thinking: 0,
    error: 0,
  };
  let activeControllerCount = 0;
  let runtimeCount = 0;
  let notificationCount = 0;
  for (const controller of state.controllers) {
    if (classifyControllerLifecycle(controller) !== 'idle') {
      activeControllerCount = Math.min(
        MAX_STATE_SUMMARY_COUNT,
        activeControllerCount + 1,
      );
    }
    if (controller.generation > 0) {
      runtimeCount = Math.min(MAX_STATE_SUMMARY_COUNT, runtimeCount + 1);
    }
    for (const message of controller.messages) {
      if (message.kind === 'compaction') continue;
      if (message.kind === 'notice') {
        notificationCount = Math.min(
          MAX_STATE_SUMMARY_COUNT,
          notificationCount + 1,
        );
        continue;
      }
      const countedKind = message.kind === 'skill' ? 'user' : message.kind;
      transcriptCounts[countedKind] = Math.min(
        MAX_STATE_SUMMARY_COUNT,
        transcriptCounts[countedKind] + 1,
      );
    }
  }
  const controller = activeController.value;
  return {
    controllerCount: Math.min(
      MAX_STATE_SUMMARY_COUNT,
      state.controllers.length,
    ),
    runtimeCount,
    activeControllerCount,
    notificationCount,
    dialogCount: Math.min(
      MAX_STATE_SUMMARY_COUNT,
      state.extensionDialogs.length,
    ),
    transcriptCounts,
    draftBucket: draftLengthBucket(controller?.draft.length ?? 0),
    scope: {
      sessionId: controller?.sessionId,
      controllerId: controller?.key,
    },
  };
}

export type {
  WorkspaceSnapshot,
  ProjectSummary,
  SessionSummary,
  ArchivedSessionEntry,
  RemoteDirectoryEntry,
  RemoteDirectoryListing,
  ExtensionDialogMethod,
  ExtensionDialog,
  SessionIndicator,
  EphemeralSession,
  PendingPrompt,
  SessionController,
  ControllerLifecyclePatch,
  RemoteRetry,
  RemoteDialogMode,
  RemoteDialogStep,
  RemoteDirectoryChoice,
  StateSnapshot,
  TranscriptKindCounts,
  FeedbackAction,
  FeedbackIncident,
  RetryPresentation,
};

export {
  effortLabels,
  indicatorLabels,
  state,
  replacementProbeDelays,
  abortAcknowledgeDelay,
  idleRuntimeLimit,
  extensionDialogTimeouts,
  replacementProbeTimers,
  abortProbeTimers,
  emptyMessages,
  emptyModels,
  emptyEfforts,
  emptyCommands,
  activeProject,
  projectActionsDisabled,
  activeSession,
  activeController,
  messages,
  draft,
  retryPresentation,
  activeFeedback,
  streaming,
  activeQueue,
  queueFeedback,
  queueBusy,
  queueFailedDrafts,
  canQueue,
  compacting,
  stopping,
  promptSubmitting,
  inProgressSessionCount,
  models,
  efforts,
  commands,
  activeExtensionDialog,
  currentModelProvider,
  currentModelId,
  currentEffort,
  canDraft,
  sessionLoading,
  canCompose,
  canReconnectRemote,
  sessionTitle,
  currentModelLabel,
  currentEffortLabel,
  settingsDisabled,
  canRenameSession,
  isProjectRemoving,
  canArchiveSession,
  projectSessions,
  archivedSessionEntries,
  sessionLastActive,
  sessionLastUserMessageAt,
  sessionSortAt,
  isSessionSelected,
  sessionIndicator,
  sessionTooltipStatus,
  expandedRelativeTime,
  isSessionUnread,
  markSessionUnread,
  markSessionRead,
  indicatorLabel,
  projectIndicator,
  setActiveSessionView,
  createPhantomSession,
  workspaceContainsSession,
  ensureController,
  stashInterruptedQueueDrafts,
  inheritControllerSettings,
  createController,
  controllerForSession,
  controllerByKey,
  controllerByRuntimeId,
  ephemeralSession,
  ephemeralSessionByController,
  isControllerSelected,
  controllerHasPendingDialog,
  runtimeAvailable,
  touchController,
  markUserMessageSubmitted,
  relativeTimestamp,
  draftTitle,
  sessionTitleMarkdown,
  tooltipTitleMarkdown,
  normalizeSessionName,
  commandOption,
  normalizeEffort,
  nextRequestId,
  nextControllerKey,
  firstUserMessage,
  clearActiveSession,
  clearRemoteDirectoryBrowser,
  applyRemoteDirectoryListing,
  presentRemoteConnectionError,
  clearRemoteRetry,
  finishRemoteConnection,
  setControllerError,
  clearControllerFeedback,
  setWorkspaceError,
  acknowledgeFeedback,
  reopenRemoteFeedback,
  resolveRemoteFeedback,
  classifyControllerLifecycle,
  setControllerLifecycle,
  sessionWorkInProgress,
  buildStateSnapshot,
};
