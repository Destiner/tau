import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';
import { effectScope, shallowRef, type ShallowRef } from 'vue';

import sessionControllerFixture from '../../tests/support/session-controller';
import { projectTrustRevision } from '../lib/pi/runtime/project-trust';

import type { ProjectSummary, SessionController } from './state';
import useProjectTrust from './useProjectTrust';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../lib/pi/ownership', () => ({
  piOwnerArgs: (): { ownerId: string } => ({ ownerId: 'owner' }),
}));

const target = { folder: '/tmp/project', destination: null };
const offer = {
  target,
  saved: false,
  exact: false,
  inspectionToken: 'offer-token',
};
const saved = { target, saved: true, exact: true, inspectionToken: null };
const trigger = { isConnected: true } as HTMLButtonElement;
const native = vi.mocked(invoke);

function liveController(id: string, generation: number): SessionController {
  const sessionFile = `/tmp/${id}.jsonl`;
  const identity = { sessionId: id, sessionFile };
  return sessionControllerFixture({
    key: id,
    runtimeId: `runtime-${id}`,
    sessionId: id,
    sessionPath: sessionFile,
    ready: true,
    generation,
    projectTrust: {
      generation,
      identity,
      report: { ...identity, generation, trusted: false, cwd: target.folder },
    },
  });
}

function mount(): {
  controller: ShallowRef<SessionController | undefined>;
  project: ShallowRef<ProjectSummary | undefined>;
  ui: ReturnType<typeof useProjectTrust>;
  stop: () => void;
} {
  const controller = shallowRef<SessionController | undefined>(
    liveController('first', 1),
  );
  const project = shallowRef<ProjectSummary | undefined>({
    path: target.folder,
    workingDirectory: target.folder,
    name: 'project',
    collapsed: false,
    selected: true,
    sessions: [],
  });
  const scope = effectScope();
  return {
    controller,
    project,
    ui: scope.run(() => useProjectTrust(controller, project))!,
    stop: (): void => scope.stop(),
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => native.mockReset());

it('drops a late inspection after switching sessions or removing the project', async () => {
  const { controller, project, ui, stop } = mount();
  const first = deferred<unknown>();
  const second = deferred<unknown>();
  native.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  try {
    ui.show(trigger);
    expect(ui.phase.value.kind).toBe('checking');
    controller.value = liveController('second', 2);
    expect(ui.open.value).toBe(false);
    expect(ui.returnFocus()).toBeUndefined();
    ui.show(trigger);
    first.resolve(offer);
    await first.promise;
    expect(ui.phase.value.kind).toBe('checking');
    project.value = undefined;
    second.resolve(offer);
    await second.promise;
    ui.grant();
    expect(ui.open.value).toBe(false);
    expect(native.mock.calls.map(([command]) => command)).toEqual([
      'inspect_project_trust',
      'inspect_project_trust',
    ]);
  } finally {
    stop();
  }
});

it('waits for an in-flight grant before reinspecting from another session', async () => {
  const { controller, ui, stop } = mount();
  const response = deferred<unknown>();
  native
    .mockResolvedValueOnce(offer)
    .mockReturnValueOnce(response.promise)
    .mockResolvedValueOnce(saved);
  try {
    ui.show(trigger);
    await vi.waitFor(() => expect(ui.phase.value.kind).toBe('offer'));
    ui.grant();
    ui.grant();
    await vi.waitFor(() => expect(native).toHaveBeenCalledTimes(2));
    expect(native).toHaveBeenLastCalledWith('grant_project_trust', {
      projectPath: target.folder,
      runtimeCwd: target.folder,
      expectedTarget: target,
      inspectionToken: 'offer-token',
      runtime: {
        ownerId: 'owner',
        generation: 1,
        runtimeId: 'runtime-first',
        sessionId: 'first',
        sessionFile: '/tmp/first.jsonl',
      },
    });
    controller.value = liveController('second', 2);
    expect(ui.open.value).toBe(false);
    ui.show(trigger);
    expect(native).toHaveBeenCalledTimes(2);
    response.resolve(saved);
    await vi.waitFor(() => expect(ui.phase.value.kind).toBe('saved'));
    expect(ui.open.value).toBe(true);
    expect(ui.untrusted.value).toBe(true);
    expect(native).toHaveBeenCalledTimes(3);
    expect(projectTrustRevision()).toBeDefined();
  } finally {
    stop();
  }
});
