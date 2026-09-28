import { archiveRevision } from '../../../composables/archive-revision';
import {
  createPhantomSession,
  draftTitle,
  sessionTitleMarkdown,
  ephemeralSessionByController,
  firstUserMessage,
  isControllerSelected,
  setControllerError,
  setWorkspaceError,
  setControllerLifecycle,
  state,
  workspaceContainsSession,
  type SessionSummary,
  type SessionController,
} from '../../../composables/state';
import { errorCopy } from '../../error-copy';
import { invokeTraced } from '../../telemetry';
import { TraceContext } from '../../telemetry/trace-context';

interface ConnectedSessionIdentity {
  generation: number;
  projectPath: string;
  sessionId: string;
  sessionPath: string;
  sessionName: string | null;
  lastUserMessageAt: number | null;
  requiresMaterialization: boolean;
}

const staleRegistration = Symbol('stale-registration');

function captureConnectedSessionIdentity(
  controller: SessionController,
): ConnectedSessionIdentity {
  const ephemeral = ephemeralSessionByController(controller.key);
  return {
    generation: controller.generation,
    projectPath: controller.projectPath,
    sessionId: controller.sessionId,
    sessionPath: controller.sessionPath,
    sessionName: controller.sessionName || firstUserMessage(controller) || null,
    lastUserMessageAt:
      controller.lastUserMessageAt > 0 ? controller.lastUserMessageAt : null,
    requiresMaterialization: Boolean(
      ephemeral && !workspaceContainsSession(controller),
    ),
  };
}

function controllerIdentityMatches(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): boolean {
  return (
    !controller.disposed &&
    controller.generation === identity.generation &&
    controller.projectPath === identity.projectPath &&
    controller.sessionId === identity.sessionId &&
    controller.sessionPath === identity.sessionPath
  );
}

function connectedSessionIdentityMatches(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): boolean {
  return (
    !controller.phantom &&
    controllerIdentityMatches(controller, identity) &&
    (!identity.requiresMaterialization || controller.materializationVerified)
  );
}

function hasMeaningfulAssistantActivity(
  controller: SessionController,
): boolean {
  return controller.messages.some(
    (message) =>
      (message.kind === 'assistant' ||
        message.kind === 'thinking' ||
        message.kind === 'tool') &&
      Boolean(message.text.trim()),
  );
}

function shouldRegisterMaterializedPredecessor(
  controller: SessionController,
): boolean {
  return (
    !controller.phantom &&
    !workspaceContainsSession(controller) &&
    controller.postSettlementHydration &&
    (controller.settledAssistantActivity ||
      hasMeaningfulAssistantActivity(controller))
  );
}

interface RegistrationMutation {
  identityKey: string;
  token: symbol;
}

const latestSessionRegistrationMutations = new Map<string, symbol>();

const sessionRegistrationFlights = new Map<
  string,
  Promise<SessionSummary | null>
>();

function registrationArchiveSnapshot(identity: ConnectedSessionIdentity): {
  revision: number;
  archived?: boolean;
} {
  return {
    revision: archiveRevision(identity.projectPath, identity.sessionId),
    archived: state.workspace?.projects
      .find((project) => project.path === identity.projectPath)
      ?.sessions.find((session) => session.id === identity.sessionId)?.archived,
  };
}

function mergeRegisteredSession(
  projectPath: string,
  session: SessionSummary | null,
  archivedAtDispatch?: { revision: number; archived?: boolean },
): boolean {
  if (!session) return false;
  const project = state.workspace?.projects.find(
    (entry) => entry.path === projectPath,
  );
  if (!project) return false;
  const index = project.sessions.findIndex((entry) => entry.id === session.id);
  // Registration only owns this row, never the other projects, sessions or
  // selection changes that may have happened while the native call was pending.
  const current = index < 0 ? undefined : project.sessions[index];
  const row = {
    ...session,
    // Archive/restore may have happened while registration was in flight.
    archived:
      current &&
      archivedAtDispatch &&
      (archiveRevision(projectPath, session.id) !==
        archivedAtDispatch.revision ||
        (archivedAtDispatch.archived !== undefined &&
          current.archived !== archivedAtDispatch.archived))
        ? current.archived
        : session.archived,
    selected: project.selected && state.activeSessionId === session.id,
  };
  if (index < 0) project.sessions.push(row);
  else Object.assign(project.sessions[index]!, row);
  return true;
}

let selectionWrite: Promise<void> = Promise.resolve();

function persistSelection(
  command: 'set_active_project' | 'set_active_session',
  args: { path: string } | { projectPath: string; sessionId: string },
  parentContext?: TraceContext,
  current: () => boolean = () => true,
): Promise<void> {
  const write = selectionWrite
    .catch(() => undefined)
    .then(async () => {
      if (current()) await invokeTraced<void>(command, args, parentContext);
    });
  selectionWrite = write.catch(() => undefined);
  return write;
}

function registrationIdentityKey(identity: ConnectedSessionIdentity): string {
  return [
    identity.generation,
    identity.projectPath,
    identity.sessionId,
    identity.sessionPath,
  ].join('\0');
}

function registrationFlightKey(
  identity: ConnectedSessionIdentity,
  adopted: boolean,
): string {
  return JSON.stringify([
    registrationIdentityKey(identity),
    identity.sessionName,
    identity.lastUserMessageAt,
    adopted,
  ]);
}

function beginRegistrationMutation(
  identity: ConnectedSessionIdentity,
): RegistrationMutation {
  const mutation = {
    identityKey: registrationIdentityKey(identity),
    token: Symbol('registration-mutation'),
  };
  latestSessionRegistrationMutations.set(mutation.identityKey, mutation.token);
  return mutation;
}

function registrationMutationIsCurrent(
  mutation: RegistrationMutation,
): boolean {
  return (
    latestSessionRegistrationMutations.get(mutation.identityKey) ===
    mutation.token
  );
}

function finishRegistrationMutation(mutation: RegistrationMutation): void {
  if (registrationMutationIsCurrent(mutation)) {
    latestSessionRegistrationMutations.delete(mutation.identityKey);
  }
}

function failPredecessorRegistration(controller: SessionController): false {
  setControllerLifecycle(controller, { syncing: false }, 'bridge_event_failed');
  setControllerError(controller, errorCopy.sessionRegistration);
  return false;
}

async function registerMaterializedPredecessor(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
): Promise<boolean> {
  const mutation = beginRegistrationMutation(identity);
  const archivedAtDispatch = registrationArchiveSnapshot(identity);
  try {
    const session = await registerSession(identity, true);
    if (
      !controllerIdentityMatches(controller, identity) ||
      !registrationMutationIsCurrent(mutation)
    ) {
      return false;
    }
    if (
      mergeRegisteredSession(identity.projectPath, session, archivedAtDispatch)
    )
      return true;
    return failPredecessorRegistration(controller);
  } catch {
    return failPredecessorRegistration(controller);
  } finally {
    finishRegistrationMutation(mutation);
  }
}

function registerSession(
  identity: ConnectedSessionIdentity,
  adopted: boolean,
  parentContext?: TraceContext,
): Promise<SessionSummary | null> {
  const key = registrationFlightKey(identity, adopted);
  const existing = sessionRegistrationFlights.get(key);
  if (existing) return existing;

  const registration = invokeTraced<SessionSummary | null>(
    'register_session',
    {
      projectPath: identity.projectPath,
      sessionId: identity.sessionId,
      sessionPath: identity.sessionPath,
      sessionName: identity.sessionName,
      lastUserMessageAt: identity.lastUserMessageAt,
      adopted,
    },
    parentContext,
  );
  sessionRegistrationFlights.set(key, registration);
  void registration
    .finally(() => {
      if (sessionRegistrationFlights.get(key) === registration) {
        sessionRegistrationFlights.delete(key);
      }
    })
    .catch(() => undefined);
  return registration;
}

// A session Tau is showing has to be selectable. Tau's registry rejecting it
// means the row and the live runtime disagree, so adopt the session Pi is
// holding and select it once more rather than leaving it unreachable.
async function selectRegisteredSession(
  controller: SessionController,
  identity: ConnectedSessionIdentity,
  mutation: RegistrationMutation,
  archivedAtDispatch?: { revision: number; archived?: boolean },
  parentContext?: TraceContext,
): Promise<void> {
  const selectionIsCurrent = (): boolean =>
    connectedSessionIdentityMatches(controller, identity) &&
    isControllerSelected(controller) &&
    registrationMutationIsCurrent(mutation);
  const select = (): Promise<void> => {
    if (!selectionIsCurrent()) {
      throw staleRegistration;
    }
    return persistSelection(
      'set_active_session',
      { projectPath: identity.projectPath, sessionId: identity.sessionId },
      parentContext,
      selectionIsCurrent,
    );
  };
  try {
    await select();
    if (!selectionIsCurrent()) throw staleRegistration;
  } catch (error) {
    if (error === staleRegistration || !selectionIsCurrent()) {
      throw staleRegistration;
    }
    const session = await registerSession(identity, true, parentContext);
    if (!selectionIsCurrent()) throw staleRegistration;
    if (
      !mergeRegisteredSession(identity.projectPath, session, archivedAtDispatch)
    )
      throw staleRegistration;
    await select();
    if (!selectionIsCurrent()) throw staleRegistration;
  }
}

async function persistExpandedProject(
  projectPath: string,
  parentContext?: TraceContext,
): Promise<void> {
  try {
    await invokeTraced<void>(
      'set_project_collapsed',
      { path: projectPath, collapsed: false },
      parentContext,
    );
  } catch {
    setWorkspaceError(errorCopy.sidebarChange);
  }
}

async function persistProjectSelection(
  projectPath: string,
  controller: SessionController,
  parentContext?: TraceContext,
): Promise<boolean> {
  try {
    if (!workspaceContainsSession(controller)) {
      await persistSelection(
        'set_active_project',
        { path: projectPath },
        parentContext,
      );
    } else {
      await persistSelection(
        'set_active_session',
        { projectPath, sessionId: controller.sessionId },
        parentContext,
      );
    }
    return true;
  } catch {
    setControllerError(controller, errorCopy.workspaceSelection);
    return false;
  }
}

// An unregistered row keeps a session reachable while Tau's registry does
// not, so a replacement has to move it onto the identity Pi handed over
// instead of leaving it pointing at the session that was swapped out.
function rebindEphemeralSession(controller: SessionController): void {
  let session = ephemeralSessionByController(controller.key);
  if (!session && !workspaceContainsSession(controller)) {
    session = createPhantomSession(controller.projectPath, controller.key);
    session.phantom = false;
    state.ephemeralSessions.push(session);
  }
  if (!session) return;
  session.id = controller.sessionId;
  session.path = controller.sessionPath;
  session.phantom = false;
  session.title = controller.sessionName || 'New Session';
  session.titleMarkdown = sessionTitleMarkdown(session.title);
  // Every workflow phase is a new session even though it reuses this object.
  // Give it fresh activity and selection instead of inheriting the first
  // phase's ordering or leaving the predecessor highlighted.
  session.lastActive = 'now';
  session.sortAt = Date.now();
  if (isControllerSelected(controller)) {
    state.activeSessionId = controller.sessionId;
    state.activeSessionPath = controller.sessionPath;
  }
}

function markPiTranscript(controller: SessionController): void {
  controller.hasPiTranscript = true;
}

function canRegisterConnectedSession(controller: SessionController): boolean {
  if (controller.phantom) return false;
  const session = ephemeralSessionByController(controller.key);
  return (
    !session ||
    workspaceContainsSession(controller) ||
    controller.materializationVerified
  );
}

function materializePendingSession(
  controller: SessionController,
  sessionId: string,
  sessionPath: string,
): void {
  const session = ephemeralSessionByController(controller.key);
  controller.sessionId = sessionId;
  controller.sessionPath = sessionPath;
  controller.phantom = false;
  if (session) {
    session.id = sessionId;
    session.path = sessionPath;
    session.phantom = false;
    const message = controller.pendingPrompt?.message ?? '';
    session.title = draftTitle(message);
    session.titleMarkdown = sessionTitleMarkdown(message);
  }
  if (isControllerSelected(controller)) {
    state.activeSessionId = sessionId;
    state.activeSessionPath = sessionPath;
  }
}

export {
  staleRegistration,
  captureConnectedSessionIdentity,
  controllerIdentityMatches,
  connectedSessionIdentityMatches,
  hasMeaningfulAssistantActivity,
  shouldRegisterMaterializedPredecessor,
  registrationArchiveSnapshot,
  mergeRegisteredSession,
  beginRegistrationMutation,
  registrationMutationIsCurrent,
  finishRegistrationMutation,
  registerMaterializedPredecessor,
  registerSession,
  selectRegisteredSession,
  persistExpandedProject,
  persistProjectSelection,
  rebindEphemeralSession,
  markPiTranscript,
  canRegisterConnectedSession,
  materializePendingSession,
};
