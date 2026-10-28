import {
  nextRequestId,
  normalizeEffort,
  type SessionController,
} from '../../../composables/state';
import { asRecord, stringValue } from '../transcript';

import { rpc } from './lifecycle';
import { cancelPendingPrompt } from './prompts';

const setupTimeoutMs = 10_000;
const setupFailureMessage =
  'Model setup failed. Check the model settings and retry setup.';

function setupFailureCopy(raw: unknown): string {
  const text = typeof raw === 'string' ? raw.toLowerCase() : '';
  if (
    /\bunauthori[sz]ed\b|\bauthentication (?:failed|failure|expired|required)\b|\b(?:invalid|missing|expired) (?:api[ _-]?key|credentials?|access token)\b|\bno api[ _-]?key (?:found|provided|configured|for)\b|\bnot logged in\b/.test(
      text,
    )
  )
    return 'Model setup could not authenticate. Check the Pi provider login on the machine running Pi, then retry setup.';
  if (
    /\bmodel\b[^\n]*(?:\bnot found\b|\bunavailable\b|\bnot available\b)|\bunsupported model\b/.test(
      text,
    )
  )
    return 'The selected model is unavailable. Choose another model or check the Pi provider configuration, then retry setup.';
  return setupFailureMessage;
}

type SetupFailure = NonNullable<SessionController['setupFailure']>;
type Operation = {
  cancelled: Promise<false>;
  current: () => boolean;
  cancel: () => void;
  captureSettings: () => void;
};
const operations = new WeakMap<SessionController, Operation>();
const pendingWatches = new WeakMap<
  SessionController,
  { id: string; timer: ReturnType<typeof setTimeout> }
>();
const responses = new Map<
  string,
  {
    controller: SessionController;
    method: string;
    resolve: (response?: Record<string, unknown>) => void;
  }
>();

// A timeout retires the UI waiter, not Pi's asynchronous mutation. Keep
// ownership until the backend replies, even if its operation was superseded.
const writes = new WeakMap<
  SessionController,
  Map<
    string,
    {
      generation: number;
      method: string;
      settled: Promise<void>;
      settle: () => void;
    }
  >
>();

function trackWrite(
  controller: SessionController,
  id: string,
  method: string,
): void {
  let pending = writes.get(controller);
  if (!pending) {
    pending = new Map();
    writes.set(controller, pending);
  }
  let settle!: () => void;
  const settled = new Promise<void>((resolve) => {
    settle = resolve;
  });
  pending.set(id, {
    generation: controller.generation,
    method,
    settled,
    settle,
  });
}

function waitForWrites(
  controller: SessionController,
  operation: Operation,
): Promise<boolean> | undefined {
  const pending = writes.get(controller);
  if (!pending) return;
  for (const [id, write] of pending) {
    if (write.generation !== controller.generation) {
      write.settle();
      pending.delete(id);
    }
  }
  if (!pending.size) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  return Promise.race([
    operation.cancelled,
    Promise.all([...pending.values()].map((write) => write.settled)).then(
      () => true,
    ),
    new Promise<false>((resolve) => {
      timer = setTimeout(() => resolve(false), setupTimeoutMs);
    }),
  ]).finally(() => clearTimeout(timer));
}

function clearPendingSetupWatch(
  controller: SessionController,
  id?: string,
): void {
  const watch = pendingWatches.get(controller);
  if (!watch || (id && watch.id !== id)) return;
  clearTimeout(watch.timer);
  pendingWatches.delete(controller);
}

function watchPendingSetup(controller: SessionController, id: string): void {
  clearPendingSetupWatch(controller);
  const pending = controller.pendingPrompt;
  trackWrite(
    controller,
    id,
    pending?.settingsStep === 'effort' ? 'set_thinking_level' : 'set_model',
  );
  const generation = controller.generation;
  const sessionId = controller.sessionId;
  const sessionPath = controller.sessionPath;
  pendingWatches.set(controller, {
    id,
    timer: setTimeout(() => {
      clearPendingSetupWatch(controller, id);
      if (
        !controller.disposed &&
        controller.generation === generation &&
        controller.sessionId === sessionId &&
        controller.sessionPath === sessionPath &&
        controller.pendingPrompt === pending &&
        pending?.settingsRequestId === id
      )
        failPendingSetup(controller);
    }, setupTimeoutMs),
  });
}

function cancelSetup(controller: SessionController): void {
  clearPendingSetupWatch(controller);
  operations.get(controller)?.cancel();
  operations.delete(controller);
  if (controller.setupFailure) {
    controller.setupFailure = controller.disposed
      ? undefined
      : { ...controller.setupFailure, recovering: false };
  }
}

function invalidateSetup(controller: SessionController): void {
  cancelSetup(controller);
  controller.setupFailure = undefined;
}

function handleSetupResponse(
  controller: SessionController,
  response: Record<string, unknown>,
): boolean {
  const id = stringValue(response.id);
  const pending = writes.get(controller);
  const write = pending?.get(id);
  if (
    write?.generation === controller.generation &&
    write.method === response.command
  ) {
    pending!.delete(id);
    write.settle();
  }
  if (!id.startsWith('tau-setup-')) return false;
  const waiter = responses.get(id);
  if (waiter?.controller === controller && waiter.method === response.command)
    waiter.resolve(response);
  // Retired setup reads must never enter general state/history hydration.
  return true;
}

function beginOperation(
  controller: SessionController,
  failure: SetupFailure,
): Operation {
  operations.get(controller)?.cancel();
  const generation = controller.generation;
  const sessionId = controller.sessionId;
  const sessionPath = controller.sessionPath;
  const settingRequestId = controller.pendingSettingRequestId;
  let cancelled = false;
  const settingsKey = (): string =>
    JSON.stringify([
      controller.currentModelProvider,
      controller.currentModelId,
      controller.currentModelName,
      controller.currentEffort,
    ]);
  let settings = settingsKey();
  let cancelWait!: () => void;
  const cancelledPromise = new Promise<false>((resolve) => {
    cancelWait = (): void => resolve(false);
  });
  const operation: Operation = {
    cancelled: cancelledPromise,
    captureSettings: () => {
      settings = settingsKey();
    },
    current: () =>
      !cancelled &&
      !controller.disposed &&
      settingsKey() === settings &&
      controller.generation === generation &&
      controller.sessionId === sessionId &&
      controller.sessionPath === sessionPath &&
      controller.pendingSettingRequestId === settingRequestId &&
      controller.setupFailure === failure &&
      operations.get(controller) === operation,
    cancel: () => {
      cancelled = true;
      cancelWait();
      for (const waiter of responses.values()) {
        if (waiter.controller === controller) waiter.resolve();
      }
    },
  };
  operations.set(controller, operation);
  return operation;
}

async function request(
  controller: SessionController,
  operation: Operation,
  type: string,
  fields: Record<string, unknown> = {},
): Promise<Record<string, unknown> | undefined> {
  if (!operation.current()) return;
  const id = nextRequestId('setup-' + type);
  if (type === 'set_model' || type === 'set_thinking_level')
    trackWrite(controller, id, type);
  return new Promise((resolve) => {
    const finish = (response?: Record<string, unknown>): void => {
      clearTimeout(timer);
      responses.delete(id);
      resolve(operation.current() ? response : undefined);
    };
    const timer = setTimeout(() => finish(), setupTimeoutMs);
    responses.set(id, { controller, method: type, resolve: finish });
    void rpc(controller, { id, type, ...fields }).catch(() => finish());
  });
}

async function reconcile(
  controller: SessionController,
  operation: Operation,
  expected?: {
    provider: string;
    modelId: string;
    effort?: SetupFailure['effort'];
  },
): Promise<boolean> {
  const response = await request(controller, operation, 'get_state');
  if (!operation.current()) return false;
  const data = response?.success === true ? asRecord(response.data) : undefined;
  if (!data) return false;
  const sessionId = stringValue(data.sessionId);
  const sessionPath = stringValue(data.sessionFile);
  if (
    !controller.phantom &&
    ((sessionId && sessionId !== controller.sessionId) ||
      (sessionPath && sessionPath !== controller.sessionPath))
  )
    return false;
  const model = asRecord(data.model);
  const levelsResponse = await request(
    controller,
    operation,
    'get_available_thinking_levels',
  );
  if (!operation.current()) return false;
  const levelsData =
    levelsResponse?.success === true
      ? asRecord(levelsResponse.data)
      : undefined;
  if (!levelsData || !Array.isArray(levelsData.levels)) return false;
  // Publish the snapshot atomically; a phantom picker can change local settings
  // without issuing an RPC while either read is in flight.
  controller.currentModelProvider = stringValue(model?.provider);
  controller.currentModelId = stringValue(model?.id);
  controller.currentModelName = stringValue(model?.name);
  controller.currentEffort = normalizeEffort(data.thinkingLevel);
  operation.captureSettings();
  controller.efforts =
    controller.currentModelProvider && controller.currentModelId
      ? levelsData.levels
          .map(normalizeEffort)
          .filter((level, index, levels) => levels.indexOf(level) === index)
      : [];
  return (
    !expected ||
    (controller.currentModelProvider === expected.provider &&
      controller.currentModelId === expected.modelId &&
      (expected.effort === undefined ||
        controller.efforts.includes(controller.currentEffort)))
  );
}

function failPendingSetup(
  controller: SessionController,
  refresh = true,
  rawError?: unknown,
): boolean {
  const pending = controller.pendingPrompt;
  if (!pending?.settingsRequestId) return false;
  clearPendingSetupWatch(controller);
  operations.get(controller)?.cancel();
  operations.delete(controller);
  const failure: SetupFailure = {
    step: pending.settingsStep === 'effort' ? 'effort' : 'model',
    provider: pending.selectedModelProvider,
    modelId: pending.selectedModelId,
    effort: pending.selectedEffort,
    message: setupFailureCopy(rawError),
    recovering: false,
  };
  cancelPendingPrompt(controller, setupFailureMessage, false);
  controller.setupFailure = failure;
  controller.efforts = [];
  if (!refresh) return true;
  const operation = beginOperation(controller, controller.setupFailure);
  void reconcile(controller, operation).finally(() => {
    if (operations.get(controller) === operation) operations.delete(controller);
  });
  return true;
}

function failSetupConnection(controller: SessionController): boolean {
  if (failPendingSetup(controller, false)) return true;
  if (!controller.setupFailure || !operations.has(controller)) return false;
  operations.get(controller)?.cancel();
  operations.delete(controller);
  controller.setupFailure = { ...controller.setupFailure, recovering: false };
  controller.efforts = [];
  return true;
}

async function recover(
  controller: SessionController,
  settings: Pick<SetupFailure, 'provider' | 'modelId' | 'effort'>,
  retryEffort = true,
): Promise<void> {
  if (controller.disposed || !controller.generation || !controller.setupFailure)
    return;
  controller.setupFailure = {
    ...settings,
    step: controller.setupFailure.step,
    message: setupFailureMessage,
    recovering: true,
  };
  const failure = controller.setupFailure;
  const operation = beginOperation(controller, failure);
  let confirmed = false;
  let message = failure.message;
  try {
    // Do not apply or confirm newer settings while an older Pi write can
    // still mutate them. A missing reply stays gated until settlement or a
    // new runtime generation; read-only reconciliation cannot prove safety.
    const settlement = waitForWrites(controller, operation);
    if ((settlement && !(await settlement)) || !operation.current()) return;
    const modelAlreadyConfirmed =
      !settlement &&
      retryEffort &&
      failure.step === 'effort' &&
      controller.currentModelProvider === settings.provider &&
      controller.currentModelId === settings.modelId;
    const model = modelAlreadyConfirmed
      ? { success: true }
      : await request(controller, operation, 'set_model', {
          provider: settings.provider,
          modelId: settings.modelId,
        });
    if (!operation.current()) return;
    if (model?.success === true && !retryEffort) {
      confirmed = await reconcile(controller, operation, {
        provider: settings.provider,
        modelId: settings.modelId,
      });
    } else if (model?.success === true) {
      const effort = await request(
        controller,
        operation,
        'set_thinking_level',
        { level: settings.effort },
      );
      if (!operation.current()) return;
      if (effort?.success !== true) message = setupFailureCopy(effort?.error);
      confirmed =
        (await reconcile(controller, operation, settings)) &&
        effort?.success === true;
    } else {
      message = setupFailureCopy(model?.error);
      await reconcile(controller, operation);
    }
  } finally {
    if (operation.current()) {
      controller.setupFailure = confirmed
        ? undefined
        : { ...failure, message, recovering: false };
    } else if (controller.setupFailure === failure) {
      controller.setupFailure = { ...failure, recovering: false };
    }
    if (operations.get(controller) === operation) operations.delete(controller);
  }
}

async function retrySetup(controller: SessionController): Promise<void> {
  const failure = controller.setupFailure;
  if (!failure || failure.recovering) return;
  await recover(controller, failure);
}

async function recoverSetupModel(
  controller: SessionController,
  provider: string,
  modelId: string,
): Promise<void> {
  const failure = controller.setupFailure;
  if (!failure) return;
  await recover(
    controller,
    {
      provider,
      modelId,
      effort: failure.effort,
    },
    false,
  );
}

export {
  retrySetup,
  recoverSetupModel,
  failPendingSetup,
  failSetupConnection,
  handleSetupResponse,
  watchPendingSetup,
  clearPendingSetupWatch,
  invalidateSetup,
  cancelSetup,
};
