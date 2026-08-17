/*
 * Tau's store: the reactive workspace state, the session controllers and
 * ephemeral sessions they drive, and the pure selectors over them. Kept
 * separate from the composable so the store can be shared with the Pi
 * runtime layer without a cycle.
 */
import { computed, reactive } from 'vue';

import type { CommandOption } from '../lib/commands';
import { scopeModels } from '../lib/pi/model-scope';
import type { ModelOption, ThinkingLevel } from '../lib/pi/model-scope';
import type { TranscriptEntry } from '../lib/pi/transcript';
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
  lastActive: string;
  lastUserMessageAt: number;
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
  timeout?: number;
  controllerKey: string;
  runtimeId: string;
  generation: number;
  projectName: string;
  sessionName: string;
  /** Base for the file paths in the text; absent when the project is remote. */
  workingDirectory?: string;
}

type ExtensionNotificationType = 'info' | 'warning' | 'error';

interface ExtensionNotification {
  key: string;
  message: string;
  type: ExtensionNotificationType;
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
  xhigh: 'Extra high',
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
}

interface PendingPrompt {
  message: string;
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
  stopping: boolean;
  starting: boolean;
  working: boolean;
  unread: boolean;
  lastUserMessageAt: number;
  messages: TranscriptEntry[];
  /** Failures Pi reports as events only; its message list never carries them. */
  localErrors: string[];
  draft: string;
  status: string;
  currentModelProvider: string;
  currentModelId: string;
  currentModelName: string;
  currentEffort: ThinkingLevel;
  pendingEffort: ThinkingLevel | '';
  models: ModelOption[];
  modelScope: string[];
  efforts: ThinkingLevel[];
  commands: CommandOption[];
  commandsLoaded: boolean;
  pendingPrompt?: PendingPrompt;
  pendingSessionRename?: PendingSessionRename;
  bootstrapStateRequestId: string;
  bootstrapSessionPath: string;
  runStateRequestId: string;
  startMessagesRequestId: string;
  commandPromptRequestId: string;
  commandSyncRequestId: string;
  replacementProbeRequestId: string;
  abortProbeRequestId: string;
  connectingRemote: boolean;
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
  activeProjectPath: '',
  activeSessionId: '',
  activeSessionPath: '',
  activeControllerKey: '',
  workspaceStatus: '',
  controllers: [] as SessionController[],
  ephemeralSessions: [] as EphemeralSession[],
  extensionDialogs: [] as ExtensionDialog[],
  extensionNotifications: [] as ExtensionNotification[],
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
  requestSequence: 0,
});

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
const extensionDialogTimeouts = new Map<
  string,
  ReturnType<typeof setTimeout>
>();
const replacementProbeTimers = new Map<
  string,
  ReturnType<typeof setTimeout>[]
>();
const abortProbeTimers = new Map<string, ReturnType<typeof setTimeout>>();
const extensionNotificationTimeouts = new Map<
  string,
  ReturnType<typeof setTimeout>
>();

const emptyMessages: TranscriptEntry[] = [];
const emptyModels: ModelOption[] = [];
const emptyEfforts: ThinkingLevel[] = [];
const emptyCommands: CommandOption[] = [];

const activeProject = computed(() =>
  state.workspace?.projects.find(
    (project) => project.path === state.activeProjectPath,
  ),
);

const activeSession = computed(() =>
  activeProject.value
    ? projectSessions(activeProject.value).find(
        (session) => session.id === state.activeSessionId,
      )
    : undefined,
);

const activeController = computed(() =>
  state.controllers.find(
    (controller) => controller.key === state.activeControllerKey,
  ),
);

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
    if (session?.phantom) session.title = draftTitle(value);
  },
});
const status = computed(
  () => activeController.value?.status || state.workspaceStatus,
);
const streaming = computed(() => activeController.value?.streaming === true);
const stopping = computed(() => activeController.value?.stopping === true);
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
const extensionNotifications = computed(() => state.extensionNotifications);
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
  Boolean(activeProject.value && activeSession.value && activeController.value),
);

/**
 * A saved session has nothing to show until its runtime hydrates the
 * transcript, so the pane reports loading instead of rendering the empty
 * session composer over a session that already holds messages.
 */
const sessionLoading = computed(() => {
  const controller = activeController.value;
  if (!controller || controller.phantom) return false;
  if (controller.messages.length > 0 || controller.status) return false;
  return !controller.ready || controller.starting || controller.syncing;
});

const canCompose = computed(() => {
  const controller = activeController.value;
  if (!activeProject.value || !activeSession.value || !controller) return false;
  if (controller.starting) return false;
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
    'New session'
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
    controller.starting
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
    !controller.pendingSessionRename,
  );
});

function canArchiveSession(
  project: ProjectSummary,
  session: SessionSummary,
): boolean {
  return ephemeralSession(project.path, session.id)?.phantom !== true;
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
      sessionLastUserMessageAt(project.path, right) -
      sessionLastUserMessageAt(project.path, left)
    );
  });
}

function sessionLastActive(
  project: ProjectSummary,
  session: SessionSummary,
): string {
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
  const selected = isSessionSelected(project, session);
  if (!selected && controllerHasPendingDialog(controller)) return 'new';
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

function projectIndicator(project: ProjectSummary): SessionIndicator {
  if (!project.collapsed) return '';
  const indicators = new Set(
    projectSessions(project).map((session) =>
      sessionIndicator(project, session),
    ),
  );
  for (const indicator of ['new', 'draft', 'working'] as const) {
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
    title: 'New session',
    lastActive: 'now',
    lastUserMessageAt: 0,
    archived: false,
    selected: true,
    projectPath,
    controllerKey,
    createdAt: now * 1000 + phantomSequence,
    phantom: true,
  };
}

function workspaceContainsSession(controller: SessionController): boolean {
  return (
    state.workspace?.projects
      .find((project) => project.path === controller.projectPath)
      ?.sessions.some((session) => session.id === controller.sessionId) === true
  );
}

function ensureController(
  project: ProjectSummary,
  session: SessionSummary,
): SessionController {
  return (
    controllerForSession(project.path, session.id) ??
    createController(project, session, nextControllerKey())
  );
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
    sessionName: session.title === 'New session' ? '' : session.title,
    phantom,
    generation: 0,
    ready: false,
    streaming: false,
    stopping: false,
    starting: false,
    working: false,
    unread: false,
    lastUserMessageAt: session.lastUserMessageAt,
    messages: [],
    localErrors: [],
    draft: '',
    status: '',
    currentModelProvider: '',
    currentModelId: '',
    currentModelName: '',
    currentEffort: 'off',
    pendingEffort: '',
    models: [],
    modelScope: [],
    efforts: [],
    commands: [],
    commandsLoaded: false,
    pendingPrompt: undefined,
    pendingSessionRename: undefined,
    bootstrapStateRequestId: '',
    bootstrapSessionPath: '',
    runStateRequestId: '',
    startMessagesRequestId: '',
    commandPromptRequestId: '',
    commandSyncRequestId: '',
    replacementProbeRequestId: '',
    abortProbeRequestId: '',
    connectingRemote: false,
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
  return normalizeSessionName(value) || 'New session';
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
  return (
    controller?.messages.find((message) => message.kind === 'user')?.text ?? ''
  );
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
  error: unknown,
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
  controller.status = errorMessage(error);
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
  state.remoteConnectionError = controller.status;
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function setControllerError(
  controller: SessionController,
  error: unknown,
): void {
  controller.status = errorMessage(error);
}

function setActiveError(error: unknown): void {
  const controller = activeController.value;
  if (controller) setControllerError(controller, error);
  else state.workspaceStatus = errorMessage(error);
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
      transcriptCounts[message.kind] = Math.min(
        MAX_STATE_SUMMARY_COUNT,
        transcriptCounts[message.kind] + 1,
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
    notificationCount: Math.min(
      MAX_STATE_SUMMARY_COUNT,
      state.extensionNotifications.length,
    ),
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
  RemoteDirectoryEntry,
  RemoteDirectoryListing,
  ExtensionDialogMethod,
  ExtensionDialog,
  ExtensionNotificationType,
  ExtensionNotification,
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
  extensionNotificationTimeouts,
  emptyMessages,
  emptyModels,
  emptyEfforts,
  emptyCommands,
  activeProject,
  activeSession,
  activeController,
  messages,
  draft,
  status,
  streaming,
  stopping,
  models,
  efforts,
  commands,
  activeExtensionDialog,
  extensionNotifications,
  currentModelProvider,
  currentModelId,
  currentEffort,
  canDraft,
  sessionLoading,
  canCompose,
  sessionTitle,
  currentModelLabel,
  currentEffortLabel,
  settingsDisabled,
  canRenameSession,
  canArchiveSession,
  projectSessions,
  sessionLastActive,
  sessionLastUserMessageAt,
  isSessionSelected,
  sessionIndicator,
  isSessionUnread,
  markSessionUnread,
  markSessionRead,
  indicatorLabel,
  projectIndicator,
  setActiveSessionView,
  createPhantomSession,
  workspaceContainsSession,
  ensureController,
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
  errorMessage,
  setControllerError,
  setActiveError,
  classifyControllerLifecycle,
  setControllerLifecycle,
  buildStateSnapshot,
};
