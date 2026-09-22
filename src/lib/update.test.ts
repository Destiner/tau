import { describe, expect, it, vi } from 'vitest';
import { watch } from 'vue';

import {
  CHECK_FOR_UPDATES_EVENT,
  CHECK_INTERVAL_MS,
  createUpdateService,
  UPDATE_PROGRESS_EVENT,
  UPDATE_STATUS_EVENT,
  updateFailureDescription,
  type UpdateDependencies,
  type UpdateService,
} from './update';

const current = { status: 'current' } as const;
const available = {
  status: 'available',
  version: '0.2.0',
  operationId: 7,
} as const;
const preparedSnapshot = {
  supported: true,
  status: 'prepared',
  operationId: 7,
  manual: false,
  candidate: { version: '0.2.0' },
} as const;

class InvocationReject {
  readonly value: unknown;

  constructor(value: unknown) {
    this.value = value;
  }
}

interface TestHarness {
  service: UpdateService;
  calls: Array<{ command: string; args?: Record<string, unknown> }>;
  respond(command: string, ...values: unknown[]): void;
  advance(ms: number): void;
  emit(event: string, payload?: unknown): void;
}

function harness(): TestHarness {
  let now = 1_000;
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  const responses = new Map<string, Array<unknown>>([
    ['get_dismissed_update_version', [null]],
    ['update_snapshot', [{ supported: false, status: 'idle' }]],
  ]);
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const dependencies: UpdateDependencies = {
    clock: { now: () => now },
    invoke: vi.fn(
      async <T>(command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        const response = responses.get(command)?.shift();
        if (response instanceof Error) throw response;
        if (response instanceof InvocationReject) throw response.value;
        return response as T;
      },
    ) as UpdateDependencies['invoke'],
    listen: vi.fn(async (event, handler) => {
      listeners.set(event, handler as (event: { payload: unknown }) => void);
      return () => listeners.delete(event);
    }),
  };
  return {
    service: createUpdateService(dependencies),
    calls,
    respond(command: string, ...values: unknown[]): void {
      responses.set(command, values);
    },
    advance(ms: number): void {
      now += ms;
    },
    emit(event: string, payload?: unknown): void {
      listeners.get(event)?.({ payload });
    },
  };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe('update service', () => {
  it('checks in the background without blocking initialization', async () => {
    const test = harness();
    test.respond('update_snapshot', { supported: true, status: 'idle' });
    test.respond('check_for_update', available);

    expect(test.service.initialize()).toBeUndefined();
    await flush();

    expect(test.service.state).toMatchObject({
      phase: 'available',
      version: '0.2.0',
    });
    expect(test.calls.map(({ command }) => command)).toEqual([
      'get_dismissed_update_version',
      'update_snapshot',
      'check_for_update',
    ]);
  });

  it('keeps unsupported builds unavailable without a failed check mark', async () => {
    const test = harness();
    test.service.initialize();
    await flush();
    expect(test.service.state.phase).toBe('unavailable');
    expect(test.service.state.failureUnread).toBe(false);
  });

  it('does not let the initial snapshot overwrite a newer check', async () => {
    const test = harness();
    let finishSnapshot!: (value: typeof preparedSnapshot) => void;
    test.respond(
      'update_snapshot',
      new Promise<typeof preparedSnapshot>(
        (resolve) => (finishSnapshot = resolve),
      ),
    );
    test.respond('check_for_update', current);

    test.service.initialize();
    await test.service.check(true);
    finishSnapshot(preparedSnapshot);
    await flush();

    expect(test.service.state).toMatchObject({
      phase: 'current',
      version: undefined,
    });
  });

  it('reveals the mounted status surface for a native menu check', async () => {
    const test = harness();
    let finishCheck!: (value: typeof current) => void;
    test.respond(
      'check_for_update',
      new Promise<typeof current>((resolve) => (finishCheck = resolve)),
    );
    test.service.initialize();
    await flush();

    test.emit(CHECK_FOR_UPDATES_EVENT);
    expect(test.service.state).toMatchObject({
      revealToken: 1,
      phase: 'checking',
    });

    finishCheck(current);
    await flush();
    expect(test.service.state.phase).toBe('current');
  });

  it('queues one native menu reveal until a status surface consumes it', async () => {
    const test = harness();
    test.respond('check_for_update', current, current);
    test.service.initialize();
    await flush();

    test.emit(CHECK_FOR_UPDATES_EVENT);
    await flush();

    let revealCount = 0;
    const mountStatus = (): (() => void) =>
      watch(
        () => test.service.state.revealToken,
        () => {
          if (test.service.consumeReveal()) revealCount += 1;
        },
        { immediate: true },
      );
    mountStatus()();
    const stop = mountStatus();
    expect(revealCount).toBe(1);

    test.emit(CHECK_FOR_UPDATES_EVENT);
    await flush();
    expect(revealCount).toBe(2);
    stop();
  });

  it('coalesces focus checks for six hours while menu checks bypass it', async () => {
    const test = harness();
    test.respond('check_for_update', current, current, current);

    await test.service.check();
    test.advance(CHECK_INTERVAL_MS - 1);
    test.service.handleFocus();
    await flush();
    expect(
      test.calls.filter(({ command }) => command === 'check_for_update'),
    ).toHaveLength(1);

    test.service.initialize();
    await flush();
    test.emit(CHECK_FOR_UPDATES_EVENT);
    await flush();
    expect(
      test.calls.filter(({ command }) => command === 'check_for_update'),
    ).toHaveLength(2);

    test.advance(CHECK_INTERVAL_MS);
    await test.service.check();
    expect(
      test.calls.filter(({ command }) => command === 'check_for_update'),
    ).toHaveLength(3);
  });

  it('keeps one check in flight', async () => {
    const test = harness();
    let finishCheck!: (value: unknown) => void;
    test.respond(
      'check_for_update',
      new Promise((resolve) => (finishCheck = resolve)),
    );

    const first = test.service.check(true);
    const second = test.service.check(true);
    expect(
      test.calls.filter(({ command }) => command === 'check_for_update'),
    ).toHaveLength(1);
    finishCheck(available);
    await Promise.all([first, second]);
    expect(test.service.state.phase).toBe('available');
  });

  it('persists a dismissal while a manual check reveals that version', async () => {
    const test = harness();
    test.respond('check_for_update', available, available);
    test.respond('set_dismissed_update_version', undefined);

    await test.service.check(true);
    await test.service.dismiss();
    expect(test.service.state.phase).toBe('current');
    expect(test.calls.at(-1)).toMatchObject({
      command: 'set_dismissed_update_version',
      args: { version: '0.2.0' },
    });

    await test.service.check(true);
    expect(test.service.state.phase).toBe('available');
  });

  it('tracks known and unknown progress and drops stale events', async () => {
    const test = harness();
    test.respond('update_snapshot', {
      supported: true,
      status: 'available',
      operationId: 7,
      candidate: { version: '0.2.0' },
    });
    let finishDownload!: (value: unknown) => void;
    test.respond(
      'download_update',
      new Promise((resolve) => (finishDownload = resolve)),
    );

    test.service.initialize();
    await flush();
    const download = test.service.download();
    test.emit(UPDATE_PROGRESS_EVENT, {
      operationId: 7,
      downloadedBytes: 20,
      totalBytes: 100,
      phase: 'downloading',
    });
    expect(test.service.state).toMatchObject({
      phase: 'downloading',
      downloadedBytes: 20,
      totalBytes: 100,
    });
    test.emit(UPDATE_PROGRESS_EVENT, {
      operationId: 8,
      downloadedBytes: 99,
      totalBytes: 100,
      phase: 'downloading',
    });
    expect(test.service.state.downloadedBytes).toBe(20);
    test.emit(UPDATE_PROGRESS_EVENT, {
      operationId: 7,
      downloadedBytes: 100,
      totalBytes: null,
      phase: 'verifying',
    });
    expect(test.service.state.phase).toBe('verifying');
    expect(test.service.state.totalBytes).toBe(100);

    finishDownload(undefined);
    await download;
    expect(test.service.state.phase).toBe('awaiting-confirmation');
  });

  it('settles an in-flight action from native status and ignores stale operations', async () => {
    const test = harness();
    test.respond('update_snapshot', {
      supported: true,
      status: 'available',
      operationId: 7,
      candidate: { version: '0.2.0' },
    });
    let finishDownload!: () => void;
    test.respond(
      'download_update',
      new Promise<void>((resolve) => (finishDownload = resolve)),
    );
    test.service.initialize();
    await flush();

    const download = test.service.download();
    test.emit(UPDATE_STATUS_EVENT, {
      supported: true,
      status: 'available',
      operationId: 7,
      manual: true,
      candidate: { version: '0.2.0' },
    });
    expect(test.service.state.phase).toBe('downloading');

    test.emit(UPDATE_STATUS_EVENT, {
      supported: true,
      status: 'prepared',
      operationId: 7,
      manual: true,
      candidate: { version: '0.2.0' },
    });
    expect(test.service.state.phase).toBe('awaiting-confirmation');

    test.emit(UPDATE_STATUS_EVENT, {
      supported: true,
      status: 'failed',
      operationId: 6,
      manual: true,
      errorCategory: 'downloadFailed',
    });
    expect(test.service.state.phase).toBe('awaiting-confirmation');

    finishDownload();
    await download;
    expect(test.service.state.phase).toBe('awaiting-confirmation');
  });

  it('settles installation only from its matching native failure snapshot', async () => {
    const test = harness();
    test.respond('update_snapshot', preparedSnapshot);
    let finishInstall!: () => void;
    test.respond(
      'install_update',
      new Promise<void>((resolve) => (finishInstall = resolve)),
    );
    test.service.initialize();
    await flush();

    const install = test.service.install(3, 7);
    test.emit(UPDATE_STATUS_EVENT, {
      ...preparedSnapshot,
      errorCategory: null,
    });
    expect(test.service.state.phase).toBe('installing');

    test.emit(UPDATE_STATUS_EVENT, {
      ...preparedSnapshot,
      errorCategory: 'installFailed',
    });
    expect(test.service.state).toMatchObject({
      phase: 'failure',
      failureCategory: 'installFailed',
    });

    finishInstall();
    await install;
    expect(test.service.state.phase).toBe('failure');
  });

  it('binds authorized installation to the native operation id', async () => {
    const test = harness();
    test.respond('update_snapshot', preparedSnapshot);
    test.respond('request_update_restart', null);
    test.respond('install_update', undefined);
    test.service.initialize();
    await flush();

    await test.service.requestInstall();
    expect(test.calls.at(-1)).toMatchObject({
      command: 'request_update_restart',
      args: { operationId: 7 },
    });
    await test.service.install(3, 8);
    expect(test.calls.some(({ command }) => command === 'install_update')).toBe(
      false,
    );
    await test.service.install(3, 7);
    expect(test.calls.at(-1)).toMatchObject({
      command: 'install_update',
      args: { requestId: 3, operationId: 7 },
    });
    expect(test.service.state.phase).toBe('restart-needed');
  });

  it('rechecks after a manual check invalidates an available candidate', async () => {
    const test = harness();
    test.respond(
      'check_for_update',
      available,
      new InvocationReject({
        category: 'checkFailed',
        message: 'private native details',
      }),
      { ...available, operationId: 8 },
    );

    await test.service.check(true);
    await test.service.check(true);
    expect(test.service.state).toMatchObject({
      phase: 'failure',
      version: undefined,
      failureCategory: 'checkFailed',
    });

    await test.service.download();
    expect(test.calls.at(-1)?.command).toBe('check_for_update');
    expect(
      test.calls.filter(({ command }) => command === 'download_update'),
    ).toHaveLength(0);
    expect(test.service.state).toMatchObject({
      phase: 'available',
      version: '0.2.0',
    });
  });

  it('preserves reviewed native error categories without exposing messages', async () => {
    const test = harness();
    test.respond('update_snapshot', {
      supported: true,
      status: 'failed',
      error: {
        category: 'verificationFailed',
        message: 'signature response containing private data',
      },
    });
    test.service.initialize();
    await flush();

    expect(test.service.state.failureCategory).toBe('verificationFailed');
    expect(updateFailureDescription(test.service.state.failureCategory)).toBe(
      'The update could not be verified and was not installed.',
    );
    expect(
      updateFailureDescription(test.service.state.failureCategory),
    ).not.toContain('private');
  });

  it('uses operation-specific reviewed failure copy', () => {
    expect(updateFailureDescription('checkFailed')).toContain(
      'check for updates',
    );
    expect(updateFailureDescription('downloadFailed')).toContain('downloaded');
    expect(updateFailureDescription('verificationFailed')).toContain(
      'verified',
    );
    expect(updateFailureDescription('preflightFailed')).toContain(
      'Applications',
    );
    expect(updateFailureDescription('authorizationExpired')).toContain(
      'expired',
    );
    expect(updateFailureDescription('installFailed')).toContain('installed');
  });

  it('shows one failure mark per failed attempt and clears it on acknowledgement', async () => {
    const test = harness();
    test.respond('check_for_update', current, new Error('private details'));

    await test.service.check(true);
    await test.service.check(true);
    expect(test.service.state).toMatchObject({
      phase: 'failure',
      failureUnread: true,
    });
    test.service.acknowledgeFailure();
    expect(test.service.state.failureUnread).toBe(false);
    test.service.acknowledgeFailure();
    expect(test.service.state.failureUnread).toBe(false);

    test.respond('check_for_update', new Error('new private details'));
    await test.service.check(true);
    expect(test.service.state.failureUnread).toBe(true);
  });
});
