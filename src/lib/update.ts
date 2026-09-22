import type { UnlistenFn } from '@tauri-apps/api/event';
import { listen } from '@tauri-apps/api/event';
import { computed, reactive, readonly, type ComputedRef } from 'vue';

import { invokeTraced } from './telemetry';

const UPDATE_PROGRESS_EVENT = 'tau://update-progress';
const UPDATE_STATUS_EVENT = 'tau://update-status';
const CHECK_FOR_UPDATES_EVENT = 'tau://check-for-updates';
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
const SCHEDULER_RETRY_MS = 60 * 1000;

type UpdateErrorCategory =
  | 'unavailable'
  | 'unsupported'
  | 'busy'
  | 'checkFailed'
  | 'downloadFailed'
  | 'verificationFailed'
  | 'preflightFailed'
  | 'authorizationExpired'
  | 'installFailed';

type UpdatePhase =
  | 'unavailable'
  | 'checking'
  | 'current'
  | 'available'
  | 'downloading'
  | 'verifying'
  | 'awaiting-confirmation'
  | 'installing'
  | 'restart-needed'
  | 'failure';

interface UpdateSnapshot {
  phase: UpdatePhase;
  version?: string;
  downloadedBytes: number;
  totalBytes?: number;
  failureCategory?: UpdateErrorCategory;
  failureUnread: boolean;
  revealToken: number;
}

interface NativeUpdateError {
  category?: unknown;
}

type NativeUpdateStatus =
  | 'idle'
  | 'checking'
  | 'upToDate'
  | 'available'
  | 'downloading'
  | 'prepared'
  | 'installing'
  | 'restartNeeded'
  | 'failed';

interface NativeUpdateSnapshot {
  supported: boolean;
  status: NativeUpdateStatus;
  operationId?: number | null;
  manual?: boolean;
  candidate?: { version: string } | null;
  error?: NativeUpdateError | null;
}

interface NativeTerminalUpdateSnapshot {
  supported: boolean;
  status: Extract<
    NativeUpdateStatus,
    'upToDate' | 'available' | 'prepared' | 'restartNeeded' | 'failed'
  >;
  operationId: number;
  manual: boolean;
  candidate?: { version: string } | null;
  errorCategory?: UpdateErrorCategory | null;
}

const TERMINAL_UPDATE_STATUSES = new Set<
  NativeTerminalUpdateSnapshot['status']
>(['upToDate', 'available', 'prepared', 'restartNeeded', 'failed']);

interface UpdateCheckResult {
  status: 'unavailable' | 'current' | 'available';
  version?: string | null;
  operationId?: number | null;
}

interface UpdateProgress {
  operationId: number;
  downloadedBytes: number;
  totalBytes?: number | null;
  phase: 'downloading' | 'verifying';
}

interface Clock {
  now(): number;
}

interface Timer {
  setTimeout(callback: () => void, delayMs: number): unknown;
  clearTimeout(handle: unknown): void;
}

type Invoke = <T>(
  command:
    | 'update_snapshot'
    | 'check_for_update'
    | 'get_dismissed_update_version'
    | 'set_dismissed_update_version'
    | 'download_update'
    | 'request_update_restart'
    | 'install_update'
    | 'restart_after_update',
  args?: Record<string, unknown>,
) => Promise<T>;

type Listen = <T>(
  event: string,
  handler: (event: { payload: T }) => void,
) => Promise<UnlistenFn>;

interface UpdateDependencies {
  clock: Clock;
  timer: Timer;
  invoke: Invoke;
  listen: Listen;
}

interface UpdateService {
  state: Readonly<UpdateSnapshot>;
  inProgress: ComputedRef<boolean>;
  initialize(): void;
  dispose(): void;
  check(manual?: boolean): Promise<void>;
  handleFocus(): void;
  download(): Promise<void>;
  dismiss(): Promise<void>;
  requestInstall(): Promise<void>;
  install(requestId: number, operationId: number): Promise<void>;
  restart(): Promise<void>;
  consumeReveal(): boolean;
  acknowledgeFailure(): void;
}

const UPDATE_ERROR_CATEGORIES = new Set<UpdateErrorCategory>([
  'unavailable',
  'unsupported',
  'busy',
  'checkFailed',
  'downloadFailed',
  'verificationFailed',
  'preflightFailed',
  'authorizationExpired',
  'installFailed',
]);

const UPDATE_FAILURE_DESCRIPTIONS: Record<UpdateErrorCategory, string> = {
  unavailable: 'Updates are unavailable in this build.',
  unsupported: 'Updates are available only from an installed macOS app.',
  busy: 'Another update operation is already in progress. Try again.',
  checkFailed:
    'Tau could not check for updates. Check your connection and try again.',
  downloadFailed:
    'The update could not be downloaded. Check your connection and try again.',
  verificationFailed: 'The update could not be verified and was not installed.',
  preflightFailed:
    'Move Tau to Applications and make sure it can be updated, then try again.',
  authorizationExpired:
    'This update request expired. Check for updates and try again.',
  installFailed: 'The update could not be installed. Try again.',
};

function isNativeTerminalUpdateSnapshot(
  value: unknown,
): value is NativeTerminalUpdateSnapshot {
  if (typeof value !== 'object' || value === null) return false;
  const snapshot = value as Partial<NativeTerminalUpdateSnapshot>;
  return (
    typeof snapshot.supported === 'boolean' &&
    typeof snapshot.status === 'string' &&
    TERMINAL_UPDATE_STATUSES.has(
      snapshot.status as NativeTerminalUpdateSnapshot['status'],
    ) &&
    Number.isSafeInteger(snapshot.operationId) &&
    (snapshot.operationId ?? 0) > 0 &&
    typeof snapshot.manual === 'boolean' &&
    (snapshot.errorCategory === undefined ||
      snapshot.errorCategory === null ||
      (typeof snapshot.errorCategory === 'string' &&
        UPDATE_ERROR_CATEGORIES.has(snapshot.errorCategory)))
  );
}

function updateErrorCategory(error: unknown): UpdateErrorCategory | undefined {
  if (typeof error !== 'object' || error === null || !('category' in error))
    return undefined;
  const category = error.category;
  return typeof category === 'string' &&
    UPDATE_ERROR_CATEGORIES.has(category as UpdateErrorCategory)
    ? (category as UpdateErrorCategory)
    : undefined;
}

function updateFailureDescription(
  category: UpdateErrorCategory | undefined,
): string {
  return UPDATE_FAILURE_DESCRIPTIONS[category ?? 'checkFailed'];
}

const defaultDependencies: UpdateDependencies = {
  clock: { now: () => Date.now() },
  timer: {
    setTimeout: (callback, delayMs) => globalThis.setTimeout(callback, delayMs),
    clearTimeout: (handle) =>
      globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>),
  },
  invoke: invokeTraced as Invoke,
  listen: listen as Listen,
};

function createUpdateService(
  dependencies: UpdateDependencies = defaultDependencies,
): UpdateService {
  const state = reactive<UpdateSnapshot>({
    phase: 'unavailable',
    downloadedBytes: 0,
    failureUnread: false,
    revealToken: 0,
  });
  const inProgress = computed(() =>
    ['downloading', 'verifying', 'installing'].includes(state.phase),
  );
  let dismissedVersion: string | undefined;
  let updateOperationId: number | undefined;
  let prepared = false;
  let initialized = false;
  let disposed = false;
  let lastCheckAt = Number.NEGATIVE_INFINITY;
  let checkFlight: Promise<void> | undefined;
  let checkTimer: unknown;
  let schedulerEnabled = false;
  let operation = 0;
  let latestNativeOperationId = 0;
  let consumedRevealToken = 0;
  const unlisteners: UnlistenFn[] = [];

  function fail(
    attempt: number,
    error: unknown,
    fallbackCategory: UpdateErrorCategory,
  ): void {
    if (disposed || attempt !== operation) return;
    if (!prepared) {
      state.version = undefined;
      updateOperationId = undefined;
    }
    state.phase = 'failure';
    state.failureCategory = updateErrorCategory(error) ?? fallbackCategory;
    state.failureUnread = true;
  }

  function applySnapshot(
    snapshot: NativeUpdateSnapshot,
    manual: boolean,
  ): void {
    if (!snapshot.supported) {
      state.phase = 'unavailable';
      state.version = undefined;
      updateOperationId = undefined;
      prepared = false;
      state.failureCategory = undefined;
      return;
    }

    const version = snapshot.candidate?.version;
    state.version = version;
    updateOperationId = snapshot.operationId ?? undefined;
    if (updateOperationId !== undefined) {
      latestNativeOperationId = Math.max(
        latestNativeOperationId,
        updateOperationId,
      );
    }
    prepared = snapshot.status === 'prepared';
    state.failureCategory = undefined;
    state.failureUnread = false;
    switch (snapshot.status) {
      case 'available':
        state.phase =
          !manual && version === dismissedVersion ? 'current' : 'available';
        break;
      case 'prepared': {
        const category = updateErrorCategory(snapshot.error);
        if (category) {
          state.phase = 'failure';
          state.failureCategory = category;
          state.failureUnread = true;
        } else {
          state.phase = 'awaiting-confirmation';
        }
        break;
      }
      case 'downloading':
        state.phase = 'downloading';
        break;
      case 'installing':
        state.phase = 'installing';
        break;
      case 'restartNeeded':
        state.phase = 'restart-needed';
        break;
      case 'checking':
        state.phase = 'checking';
        break;
      case 'failed':
        state.phase = 'failure';
        state.version = undefined;
        updateOperationId = undefined;
        prepared = false;
        state.failureCategory =
          updateErrorCategory(snapshot.error) ?? 'checkFailed';
        state.failureUnread = true;
        break;
      default:
        state.phase = 'current';
    }
  }

  function applyTerminalSnapshot(snapshot: NativeTerminalUpdateSnapshot): void {
    const operationId = snapshot.operationId;
    const settlesCurrentAction =
      updateOperationId === operationId &&
      ((['downloading', 'verifying'].includes(state.phase) &&
        (snapshot.status === 'prepared' || snapshot.status === 'failed')) ||
        (state.phase === 'installing' &&
          (snapshot.status === 'restartNeeded' ||
            snapshot.status === 'failed' ||
            (snapshot.status === 'prepared' &&
              snapshot.errorCategory !== undefined &&
              snapshot.errorCategory !== null))));
    if (
      operationId < latestNativeOperationId ||
      (operationId === latestNativeOperationId && !settlesCurrentAction)
    )
      return;

    operation += 1;
    latestNativeOperationId = operationId;
    applySnapshot(
      {
        supported: snapshot.supported,
        status: snapshot.status,
        operationId,
        manual: snapshot.manual,
        candidate: snapshot.candidate,
        error: snapshot.errorCategory
          ? { category: snapshot.errorCategory }
          : undefined,
      },
      snapshot.manual,
    );
  }

  function clearScheduledCheck(): void {
    if (checkTimer === undefined) return;
    dependencies.timer.clearTimeout(checkTimer);
    checkTimer = undefined;
  }

  function scheduleNextCheck(): void {
    if (disposed || !schedulerEnabled) return;
    clearScheduledCheck();
    const elapsed = dependencies.clock.now() - lastCheckAt;
    const checkBlocked =
      inProgress.value ||
      state.phase === 'awaiting-confirmation' ||
      state.phase === 'restart-needed';
    const delay = !Number.isFinite(elapsed)
      ? CHECK_INTERVAL_MS
      : elapsed >= CHECK_INTERVAL_MS && checkBlocked
        ? SCHEDULER_RETRY_MS
        : Math.max(0, CHECK_INTERVAL_MS - elapsed);
    checkTimer = dependencies.timer.setTimeout(() => {
      checkTimer = undefined;
      if (disposed) return;
      void check().finally(scheduleNextCheck);
    }, delay);
  }

  async function check(manual = false): Promise<void> {
    if (checkFlight) return checkFlight;
    if (
      inProgress.value ||
      state.phase === 'awaiting-confirmation' ||
      state.phase === 'restart-needed'
    )
      return;
    if (!manual && dependencies.clock.now() - lastCheckAt < CHECK_INTERVAL_MS)
      return;

    const attempt = ++operation;
    const wasUnavailable = state.phase === 'unavailable';
    clearScheduledCheck();
    lastCheckAt = dependencies.clock.now();
    state.phase = 'checking';
    state.version = undefined;
    state.failureCategory = undefined;
    state.failureUnread = false;
    updateOperationId = undefined;
    prepared = false;
    const flight = (async (): Promise<void> => {
      try {
        const result = await dependencies.invoke<UpdateCheckResult>(
          'check_for_update',
          { manual },
        );
        if (disposed || attempt !== operation) return;
        const resultOperationId = result.operationId ?? undefined;
        if (
          resultOperationId !== undefined &&
          (!Number.isSafeInteger(resultOperationId) ||
            resultOperationId <= 0 ||
            resultOperationId < latestNativeOperationId)
        )
          return;
        state.version = result.version ?? undefined;
        updateOperationId = resultOperationId;
        if (resultOperationId !== undefined) {
          latestNativeOperationId = resultOperationId;
        }
        prepared = false;
        state.failureCategory = undefined;
        state.phase =
          result.status === 'available' && result.version
            ? !manual && result.version === dismissedVersion
              ? 'current'
              : 'available'
            : result.status;
      } catch (error) {
        if (disposed || attempt !== operation) return;
        if (wasUnavailable) {
          state.phase = 'unavailable';
          state.version = undefined;
          updateOperationId = undefined;
          prepared = false;
        } else {
          fail(attempt, error, 'checkFailed');
        }
      }
    })();
    checkFlight = flight;
    try {
      await flight;
    } finally {
      if (checkFlight === flight) checkFlight = undefined;
      scheduleNextCheck();
    }
  }

  function initialize(): void {
    if (initialized) return;
    initialized = true;
    disposed = false;
    void dependencies
      .listen<UpdateProgress>(UPDATE_PROGRESS_EVENT, ({ payload }) => {
        if (
          disposed ||
          !inProgress.value ||
          (updateOperationId !== undefined &&
            payload.operationId !== updateOperationId)
        )
          return;
        updateOperationId ??= payload.operationId;
        if (payload.phase === 'verifying') state.phase = 'verifying';
        if (Number.isFinite(payload.downloadedBytes)) {
          state.downloadedBytes = Math.max(0, payload.downloadedBytes);
        }
        if (
          typeof payload.totalBytes === 'number' &&
          Number.isFinite(payload.totalBytes) &&
          payload.totalBytes > 0
        ) {
          state.totalBytes = payload.totalBytes;
        }
      })
      .then(keepListener)
      .catch(() => undefined);
    void dependencies
      .listen<unknown>(UPDATE_STATUS_EVENT, ({ payload }) => {
        if (disposed || !isNativeTerminalUpdateSnapshot(payload)) return;
        applyTerminalSnapshot(payload);
      })
      .then(keepListener)
      .catch(() => undefined);
    void dependencies
      .listen(CHECK_FOR_UPDATES_EVENT, () => {
        state.revealToken += 1;
        void check(true);
      })
      .then(keepListener)
      .catch(() => undefined);

    const initializationGeneration = operation;
    // Local state is read asynchronously, but before the network check, so an
    // unsupported or dismissed update never briefly appears as a failure or
    // as available.
    void Promise.all([
      dependencies
        .invoke<string | null>('get_dismissed_update_version')
        .catch(() => null),
      dependencies
        .invoke<NativeUpdateSnapshot>('update_snapshot')
        .catch(() => null),
    ]).then(([version, snapshot]) => {
      if (disposed) return;
      dismissedVersion = version ?? undefined;
      if (!snapshot || operation !== initializationGeneration) return;
      applySnapshot(snapshot, false);
      if (!snapshot.supported) return;
      schedulerEnabled = true;
      if (snapshot.status === 'idle' || snapshot.status === 'upToDate') {
        void check();
      } else {
        if (!Number.isFinite(lastCheckAt))
          lastCheckAt = dependencies.clock.now();
        scheduleNextCheck();
      }
    });
  }

  function keepListener(unlisten: UnlistenFn): void {
    if (disposed) unlisten();
    else unlisteners.push(unlisten);
  }

  function handleFocus(): void {
    void check();
  }

  async function download(): Promise<void> {
    if (state.phase === 'failure' && prepared) {
      await requestInstall();
      return;
    }
    if (state.phase !== 'available' && state.phase !== 'failure') return;
    const attempt = ++operation;
    if (updateOperationId === undefined) {
      await check(true);
      return;
    }
    state.phase = 'downloading';
    state.downloadedBytes = 0;
    state.totalBytes = undefined;
    state.failureCategory = undefined;
    state.failureUnread = false;
    try {
      await dependencies.invoke<void>('download_update', {
        operationId: updateOperationId,
      });
      if (disposed || attempt !== operation) return;
      prepared = true;
      state.phase = 'awaiting-confirmation';
    } catch (error) {
      fail(attempt, error, 'downloadFailed');
    }
  }

  async function dismiss(): Promise<void> {
    if (state.phase !== 'available' || !state.version) return;
    const version = state.version;
    dismissedVersion = version;
    state.phase = 'current';
    try {
      await dependencies.invoke<void>('set_dismissed_update_version', {
        version,
      });
    } catch {
      // Optimistically suppress this run. A later launch may offer it again.
    }
  }

  async function requestInstall(): Promise<void> {
    if (
      (state.phase !== 'awaiting-confirmation' && state.phase !== 'failure') ||
      updateOperationId === undefined
    )
      return;
    const attempt = ++operation;
    state.phase = 'awaiting-confirmation';
    state.failureCategory = undefined;
    state.failureUnread = false;
    try {
      await dependencies.invoke<void>('request_update_restart', {
        operationId: updateOperationId,
      });
    } catch (error) {
      if (updateErrorCategory(error) !== 'busy') prepared = false;
      fail(attempt, error, 'authorizationExpired');
    }
  }

  async function install(
    requestId: number,
    operationId: number,
  ): Promise<void> {
    if (
      state.phase !== 'awaiting-confirmation' ||
      !Number.isSafeInteger(requestId) ||
      operationId !== updateOperationId
    )
      return;
    const attempt = ++operation;
    state.phase = 'installing';
    state.failureCategory = undefined;
    state.failureUnread = false;
    try {
      await dependencies.invoke<void>('install_update', {
        requestId,
        operationId,
      });
      if (disposed || attempt !== operation) return;
      state.phase = 'restart-needed';
    } catch (error) {
      prepared = true;
      fail(attempt, error, 'installFailed');
    }
  }

  async function restart(): Promise<void> {
    if (state.phase !== 'restart-needed' || updateOperationId === undefined)
      return;
    try {
      await dependencies.invoke<void>('restart_after_update', {
        operationId: updateOperationId,
      });
    } catch (error) {
      state.failureCategory =
        updateErrorCategory(error) ?? 'authorizationExpired';
      state.failureUnread = true;
    }
  }

  function consumeReveal(): boolean {
    if (consumedRevealToken >= state.revealToken) return false;
    consumedRevealToken = state.revealToken;
    return true;
  }

  function acknowledgeFailure(): void {
    state.failureUnread = false;
  }

  function dispose(): void {
    disposed = true;
    schedulerEnabled = false;
    clearScheduledCheck();
    operation += 1;
    for (const unlisten of unlisteners.splice(0)) unlisten();
  }

  return {
    state: readonly(state) as Readonly<UpdateSnapshot>,
    inProgress,
    initialize,
    dispose,
    check,
    handleFocus,
    download,
    dismiss,
    requestInstall,
    install,
    restart,
    consumeReveal,
    acknowledgeFailure,
  };
}

const updateService = createUpdateService();

function useUpdate(): UpdateService {
  return updateService;
}

export type {
  UpdateDependencies,
  UpdateErrorCategory,
  UpdatePhase,
  UpdateService,
  UpdateSnapshot,
};

export {
  CHECK_FOR_UPDATES_EVENT,
  CHECK_INTERVAL_MS,
  createUpdateService,
  UPDATE_PROGRESS_EVENT,
  UPDATE_STATUS_EVENT,
  updateFailureDescription,
  useUpdate,
};

export default useUpdate;
