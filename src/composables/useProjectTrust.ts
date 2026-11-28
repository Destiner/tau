import { invoke } from '@tauri-apps/api/core';
import { computed, shallowRef, watch, type Ref } from 'vue';

import { piOwnerArgs } from '../lib/pi/ownership';
import {
  effectiveProjectTrust,
  markProjectTrustGranted,
} from '../lib/pi/runtime/project-trust';

import type { ProjectSummary, SessionController } from './state';

type Target = Readonly<{ folder: string; destination: string | null }>;
type TrustResult = {
  target: Target;
  saved: boolean;
  exact: boolean;
  inspectionToken: string | null;
};
type Binding = {
  trigger: HTMLButtonElement;
  controllerKey: string;
  runtimeId: string;
  projectPath: string;
  workingDirectory: string;
  connectionString: string | undefined;
  report: NonNullable<ReturnType<typeof effectiveProjectTrust>>;
};
type Phase =
  | { kind: 'checking' | 'unavailable' | 'mismatch' | 'unsupported' }
  | { kind: 'offer'; target: Target; inspectionToken: string }
  | { kind: 'saved' | 'saving' | 'unconfirmed'; target: Target };

type ProjectTrust = {
  open: Ref<boolean>;
  untrusted: Readonly<Ref<boolean>>;
  phase: Readonly<Ref<Phase>>;
  busy: Readonly<Ref<boolean>>;
  description: Readonly<Ref<string>>;
  show: (trigger: HTMLButtonElement) => void;
  close: () => void;
  recheck: () => void;
  grant: () => void;
  returnFocus: () => HTMLElement | undefined;
};

function sameTarget(first: Target, second: Target): boolean {
  return (
    first.folder === second.folder && first.destination === second.destination
  );
}

function targetText(target: Target): string {
  return target.destination
    ? `${target.folder} on ${target.destination}`
    : target.folder;
}

function errorPhase(
  error: unknown,
): 'mismatch' | 'unsupported' | 'unavailable' | 'unconfirmed' | undefined {
  if (typeof error !== 'object' || !error || !('kind' in error))
    return undefined;
  switch (error.kind) {
    case 'mismatch':
    case 'unsupported':
    case 'unavailable':
    case 'unconfirmed':
      return error.kind;
    default:
      return undefined;
  }
}

export default function useProjectTrust(
  controller: Readonly<Ref<SessionController | undefined>>,
  project: Readonly<Ref<ProjectSummary | undefined>>,
): ProjectTrust {
  const binding = shallowRef<Binding>();
  const returnOrigin = shallowRef<Binding>();
  const phase = shallowRef<Phase>({ kind: 'checking' });
  const pendingGrants = new Map<string, Promise<void>>();
  const unconfirmedTargets = new Map<string, Target>();
  const report = computed(() => effectiveProjectTrust(controller.value));
  const untrusted = computed(() => report.value?.trusted === false);
  const busy = computed(() => phase.value.kind === 'saving');
  const open = computed({
    get: () => Boolean(binding.value),
    set: (next: boolean) => {
      if (!next) close();
    },
  });

  function close(): void {
    if (!busy.value) binding.value = undefined;
  }

  function matches(owner: Binding): boolean {
    const latest = report.value;
    return (
      controller.value?.key === owner.controllerKey &&
      controller.value.runtimeId === owner.runtimeId &&
      project.value?.path === owner.projectPath &&
      project.value.workingDirectory === owner.workingDirectory &&
      project.value.connectionString === owner.connectionString &&
      latest?.trusted === false &&
      latest.generation === owner.report.generation &&
      latest.sessionId === owner.report.sessionId &&
      latest.sessionFile === owner.report.sessionFile &&
      latest.cwd === owner.report.cwd
    );
  }

  watch(
    () => binding.value && matches(binding.value),
    (valid) => {
      if (!valid) binding.value = undefined;
    },
    { flush: 'sync' },
  );

  const isCurrent = (owner: Binding): boolean =>
    binding.value === owner && matches(owner);
  function args(owner: Binding): Record<string, unknown> {
    return {
      projectPath: owner.projectPath,
      runtimeCwd: owner.report.cwd,
      runtime: {
        ...piOwnerArgs(),
        runtimeId: owner.runtimeId,
        generation: owner.report.generation,
        sessionId: owner.report.sessionId,
        sessionFile: owner.report.sessionFile,
      },
    };
  }

  async function inspect(owner: Binding): Promise<void> {
    phase.value = { kind: 'checking' };
    try {
      const pendingGrant = pendingGrants.get(owner.projectPath);
      if (pendingGrant) await pendingGrant;
      if (!isCurrent(owner)) return;
      const result = await invoke<TrustResult>(
        'inspect_project_trust',
        args(owner),
      );
      if (!isCurrent(owner)) return;
      const unconfirmed = unconfirmedTargets.get(owner.projectPath);
      if (unconfirmed) {
        unconfirmedTargets.delete(owner.projectPath);
        if (result.exact && sameTarget(unconfirmed, result.target)) {
          markProjectTrustGranted();
        }
      }
      phase.value = result.saved
        ? { kind: 'saved', target: result.target }
        : result.inspectionToken
          ? {
              kind: 'offer',
              target: result.target,
              inspectionToken: result.inspectionToken,
            }
          : { kind: 'unavailable' };
    } catch (error) {
      if (!isCurrent(owner)) return;
      const kind = errorPhase(error);
      phase.value = {
        kind:
          kind === 'unsupported' || kind === 'mismatch' ? kind : 'unavailable',
      };
    }
  }

  function show(trigger: HTMLButtonElement): void {
    const active = controller.value;
    const current = project.value;
    const trust = report.value;
    if (
      binding.value ||
      !active ||
      !current ||
      active.projectPath !== current.path ||
      trust?.trusted !== false
    )
      return;
    const owner: Binding = {
      trigger,
      controllerKey: active.key,
      runtimeId: active.runtimeId,
      projectPath: current.path,
      workingDirectory: current.workingDirectory,
      connectionString: current.connectionString,
      report: trust,
    };
    returnOrigin.value = owner;
    binding.value = owner;
    void inspect(owner);
  }

  function recheck(): void {
    const owner = binding.value;
    if (
      !owner ||
      !['unconfirmed', 'unavailable', 'mismatch'].includes(phase.value.kind)
    )
      return;
    void inspect(owner);
  }

  function grant(): void {
    const owner = binding.value;
    const current = phase.value;
    if (
      !owner ||
      !isCurrent(owner) ||
      current.kind !== 'offer' ||
      pendingGrants.has(owner.projectPath)
    )
      return;
    const { target, inspectionToken } = current;
    phase.value = { kind: 'saving', target };
    const job = async (): Promise<void> => {
      if (!isCurrent(owner)) return;
      try {
        const result = await invoke<TrustResult>('grant_project_trust', {
          ...args(owner),
          expectedTarget: target,
          inspectionToken,
        });
        if (
          !result.saved ||
          !result.exact ||
          !sameTarget(target, result.target)
        )
          throw { kind: 'unconfirmed' };
        unconfirmedTargets.delete(owner.projectPath);
        markProjectTrustGranted();
        if (isCurrent(owner)) binding.value = undefined;
      } catch (error) {
        const kind = errorPhase(error) ?? 'unconfirmed';
        if (kind === 'unconfirmed')
          unconfirmedTargets.set(owner.projectPath, target);
        if (isCurrent(owner)) {
          phase.value = kind === 'unconfirmed' ? { kind, target } : { kind };
        }
      }
    };
    const pending = Promise.resolve()
      .then(job)
      .finally(() => pendingGrants.delete(owner.projectPath));
    pendingGrants.set(owner.projectPath, pending);
  }

  function describePhase(current: Phase): string {
    switch (current.kind) {
      case 'checking':
        return 'Checking the saved trust decision…';
      case 'offer':
        return `Trusting ${targetText(current.target)} allows Pi to load project skills and run project extensions.`;
      case 'saved':
        return `Trust is already saved for ${targetText(current.target)}. This session is still untrusted. Pi checks trust again in a new process.`;
      case 'saving':
        return `Saving trust for ${targetText(current.target)}…`;
      case 'unconfirmed':
        return `Tau couldn’t confirm whether trust was saved for ${targetText(current.target)}. Recheck before trying again.`;
      case 'mismatch':
        return 'Tau couldn’t verify that the project still matches the folder used by this session.';
      case 'unsupported':
        return 'Tau can’t manage trust with this Pi installation. Run /trust in interactive Pi from the project folder.';
      case 'unavailable':
        return 'Tau couldn’t access Pi’s saved trust decision. Check that the project is reachable and try again.';
    }
  }

  function returnFocus(): HTMLElement | undefined {
    const origin = returnOrigin.value;
    return origin?.trigger.isConnected && matches(origin)
      ? origin.trigger
      : undefined;
  }

  return {
    open,
    untrusted,
    phase,
    busy,
    description: computed(() => describePhase(phase.value)),
    show,
    close,
    recheck,
    grant,
    returnFocus,
  };
}
