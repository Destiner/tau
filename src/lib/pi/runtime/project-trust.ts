import type { SessionController } from '../../../composables/state';

const projectTrustStatusKey = 'tau-internal-project-trust-v1';
const maxPathLength = 4096;
const maxStatusLength = 64 * 1024;

type ProjectTrustIdentity = {
  sessionId: string;
  sessionFile: string | null;
};

type ProjectTrustReport = Readonly<
  ProjectTrustIdentity & {
    generation: number;
    trusted: boolean;
    cwd: string;
  }
>;

type ProjectTrustRuntime = {
  generation: number;
  identity?: ProjectTrustIdentity;
  predecessor?: ProjectTrustIdentity;
  pending?: ProjectTrustReport;
  report?: ProjectTrustReport;
};

function absolutePath(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length <= maxPathLength &&
    !value.includes('\0') &&
    /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(value)
  );
}

function sessionIdentity(
  sessionId: unknown,
  sessionFile: unknown,
): ProjectTrustIdentity | undefined {
  if (
    typeof sessionId !== 'string' ||
    !sessionId.trim() ||
    sessionId.length > 256 ||
    sessionId.includes('\0') ||
    (sessionFile !== null && !absolutePath(sessionFile))
  ) {
    return undefined;
  }
  return { sessionId, sessionFile };
}

function sameIdentity(
  first: ProjectTrustIdentity | undefined,
  second: ProjectTrustIdentity | undefined,
): boolean {
  return (
    first !== undefined &&
    second !== undefined &&
    first.sessionId === second.sessionId &&
    first.sessionFile === second.sessionFile
  );
}

function hasPendingReplacement(runtime: ProjectTrustRuntime): boolean {
  return (
    runtime.pending !== undefined &&
    !sameIdentity(runtime.pending, runtime.predecessor)
  );
}

function canReceiveProjectTrust(
  controller: SessionController,
  generation: number,
): boolean {
  return (
    Number.isSafeInteger(generation) &&
    generation > (controller.projectTrustClosedThrough ?? 0) &&
    !controller.projectTrustSuspended &&
    !controller.disposed &&
    !controller.remoteConnectionTimedOut &&
    (!controller.runtimeDisconnected || controller.recoveringRuntime)
  );
}

function handleProjectTrustStatus(
  controller: SessionController,
  generation: number,
  event: Record<string, unknown>,
): void {
  const { id, statusKey, statusText, method } = event;
  if (
    method !== 'setStatus' ||
    statusKey !== projectTrustStatusKey ||
    typeof id !== 'string' ||
    !id ||
    typeof statusText !== 'string' ||
    statusText.length > maxStatusLength ||
    !canReceiveProjectTrust(controller, generation) ||
    (generation !== controller.generation &&
      !(controller.starting && generation > controller.generation))
  ) {
    return;
  }
  let payload: unknown;
  try {
    payload = JSON.parse(statusText);
  } catch {
    return;
  }
  if (!payload || typeof payload !== 'object') return;
  const { version, trusted, cwd, sessionId, sessionFile } = payload as Record<
    string,
    unknown
  >;
  const identity = sessionIdentity(sessionId, sessionFile);
  if (
    version !== 1 ||
    typeof trusted !== 'boolean' ||
    !absolutePath(cwd) ||
    !identity
  ) {
    return;
  }

  const current = controller.projectTrust;
  if (current && current.generation > generation) return;
  const runtime = current?.generation === generation ? current : { generation };
  const report = { ...identity, generation, trusted, cwd };
  if (sameIdentity(runtime.identity, report)) {
    controller.projectTrust = { ...runtime, report };
  } else if (
    !sameIdentity(runtime.predecessor, report) ||
    !hasPendingReplacement(runtime)
  ) {
    // A predecessor status can be a late delivery or a switch back. Let
    // get_state distinguish them without displacing a queued successor.
    controller.projectTrust = { ...runtime, pending: report };
  }
}

// A phantom's Tau identity precedes Pi's session ID. Confirm the private
// status against a correlated get_state, not against the sidebar row.
function observeProjectTrustSession(
  controller: SessionController,
  sessionId: unknown,
  sessionFile: unknown,
): void {
  const generation = controller.generation;
  if (!canReceiveProjectTrust(controller, generation)) return;
  const identity = sessionIdentity(sessionId, sessionFile ?? null);
  const runtime = controller.projectTrust;
  const previous = runtime?.generation === generation ? runtime : undefined;
  // Only the next identity read can confirm a pending status. Don't retain
  // it for a later visit to the same Pi session.
  const report = sameIdentity(identity, previous?.pending)
    ? previous?.pending
    : sameIdentity(identity, previous?.identity)
      ? previous?.report
      : undefined;
  const predecessor =
    identity && previous?.identity && !sameIdentity(identity, previous.identity)
      ? previous.identity
      : previous?.predecessor;
  controller.projectTrust = { generation, identity, predecessor, report };
}

function effectiveProjectTrust(
  controller: SessionController | undefined,
): ProjectTrustReport | undefined {
  const runtime = controller?.projectTrust;
  const report = runtime?.report;
  if (
    !controller ||
    !report ||
    hasPendingReplacement(runtime) ||
    !controller.ready ||
    controller.runtimeDisconnected ||
    report.generation !== controller.generation ||
    !canReceiveProjectTrust(controller, report.generation) ||
    (!controller.phantom &&
      !sameIdentity(report, {
        sessionId: controller.sessionId,
        sessionFile: controller.sessionPath || null,
      }))
  ) {
    return undefined;
  }
  return report;
}

function adoptProjectTrustGeneration(
  controller: SessionController,
  generation: number,
): void {
  if (controller.projectTrust?.generation !== generation) {
    controller.projectTrust = undefined;
  }
}

function retireProjectTrustGeneration(
  controller: SessionController,
  generation: number,
): void {
  if (!Number.isSafeInteger(generation) || generation < 0) return;
  controller.projectTrustClosedThrough = Math.max(
    generation,
    controller.projectTrustClosedThrough ?? 0,
  );
  if (
    controller.projectTrust &&
    controller.projectTrust.generation <= generation
  ) {
    controller.projectTrust = undefined;
  }
}

let catalogueRevision: symbol | undefined;

function projectTrustRevision(): symbol | undefined {
  return catalogueRevision;
}

// Pi may inherit grants through paths or aliases Tau cannot resolve. Invalidate
// future catalogue reuse conservatively without changing any live process.
function markProjectTrustGranted(): void {
  catalogueRevision = Symbol();
}

export {
  adoptProjectTrustGeneration,
  effectiveProjectTrust,
  handleProjectTrustStatus,
  markProjectTrustGranted,
  observeProjectTrustSession,
  projectTrustRevision,
  projectTrustStatusKey,
  retireProjectTrustGeneration,
  type ProjectTrustRuntime,
};
