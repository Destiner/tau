import type { Locator, Page } from '@playwright/test';

import type { PiBridgeEvent } from '../../src/lib/pi/bridge';

import { expect, test } from './fixtures';

const projectPath = '/browser-dev/projects/atlas';
const localTarget = { folder: projectPath, destination: null };
const sshTarget = {
  folder:
    '/home/dev/work/harbor/a-long-project-folder/with-another-directory/and-a-final-component',
  destination: 'dev@build.example:2222',
};

type GrantOutcome = 'saved' | 'unconfirmed' | 'saved-without-ack';

type TrustArgs = {
  projectPath: string;
  runtimeCwd: string;
  runtime: {
    ownerId: string;
    runtimeId: string;
    generation: number;
    sessionId: string;
    sessionFile: string;
  };
  inspectionToken?: string;
  expectedTarget?: typeof localTarget | typeof sshTarget;
};

declare global {
  interface Window {
    __tauProjectTrustMock: {
      calls: { command: string; args: TrustArgs }[];
      runtimeChanges: string[];
      saved: boolean;
      finishGrant?: (outcome: GrantOutcome) => void;
    };
  }
}

async function openWorkspace(page: Page, remote = false): Promise<void> {
  // Keep the sandbox running the real App and its Pi bootstrap. Inject only
  // the private Pi status and the missing native trust commands at IPC.
  await page.addInitScript(
    ({ projectPath, localTarget, sshTarget, remote }) => {
      type Invoke = (
        command: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
      type Internals = { invoke?: Invoke };
      const appWindow = window as typeof window & {
        __TAURI_INTERNALS__?: Internals;
      };
      const mock = (window.__tauProjectTrustMock = {
        calls: [],
        runtimeChanges: [],
        saved: false,
      } as Window['__tauProjectTrustMock']);
      const target = remote ? sshTarget : localTarget;
      let internals: Internals;
      Object.defineProperty(appWindow, '__TAURI_INTERNALS__', {
        configurable: true,
        get: () => internals,
        set: (next: Internals) => {
          internals = next;
          let invoke: Invoke;
          Object.defineProperty(next, 'invoke', {
            configurable: true,
            get: () => invoke,
            set: (sandboxInvoke: Invoke) => {
              invoke = async (command, args): Promise<unknown> => {
                if (
                  command === 'inspect_project_trust' ||
                  command === 'grant_project_trust'
                ) {
                  mock.calls.push({ command, args: args as TrustArgs });
                  if (command === 'grant_project_trust') {
                    const outcome = await new Promise<GrantOutcome>(
                      (resolve) => {
                        mock.finishGrant = resolve;
                      },
                    );
                    delete mock.finishGrant;
                    mock.saved = outcome !== 'unconfirmed';
                    if (outcome !== 'saved') throw { kind: 'unconfirmed' };
                  }
                  return {
                    target,
                    saved: mock.saved,
                    exact: mock.saved,
                    inspectionToken:
                      command === 'inspect_project_trust'
                        ? `inspection-${mock.calls.length}`
                        : null,
                  };
                }
                if (
                  command === 'stop_pi' ||
                  (command === 'send_pi' &&
                    (args?.request as { type: string }).type === 'prompt')
                ) {
                  mock.runtimeChanges.push(command);
                }
                if (command === 'start_pi' || command === 'start_pi_remote') {
                  mock.runtimeChanges.push(command);
                  const generation = (await sandboxInvoke(
                    command,
                    args,
                  )) as number;
                  const sessionFile = args?.sessionPath as string | null;
                  const sessionId = sessionFile
                    ?.split('/')
                    .at(-1)
                    ?.replace(/\.jsonl$/, '');
                  if (
                    sessionId === 'atlas-overview' ||
                    sessionId === 'atlas-release'
                  ) {
                    const payload: PiBridgeEvent = {
                      runtimeId: args!.runtimeId as string,
                      generation,
                      kind: 'rpc',
                      line: JSON.stringify({
                        type: 'extension_ui_request',
                        method: 'setStatus',
                        id: `trust-${generation}`,
                        statusKey: 'tau-internal-project-trust-v1',
                        statusText: JSON.stringify({
                          version: 1,
                          trusted: sessionId === 'atlas-release',
                          cwd: remote
                            ? args!.workingDirectory
                            : args!.projectPath,
                          sessionId,
                          sessionFile,
                        }),
                      }),
                    };
                    await sandboxInvoke('plugin:event|emit', {
                      event: 'pi-event',
                      payload,
                    });
                  }
                  return generation;
                }
                const result = await sandboxInvoke(command, args);
                if (remote && command === 'load_workspace') {
                  const workspace = result as {
                    projects: {
                      path: string;
                      connectionString?: string;
                      workingDirectory: string;
                    }[];
                  };
                  const project = workspace.projects.find(
                    (item) => item.path === projectPath,
                  )!;
                  project.connectionString = 'ssh -p 2222 dev@build.example';
                  project.workingDirectory = sshTarget.folder;
                }
                return result;
              };
            },
          });
        },
      });
    },
    { projectPath, localTarget, sshTarget, remote },
  );
  await page.goto('/');
  await expect(shield(page)).toBeVisible();
  await expect(composer(page)).toBeEnabled();
}

function shield(page: Page): Locator {
  return page.getByRole('button', { name: 'Untrusted Project', exact: true });
}

function trustDialog(page: Page): Locator {
  return page.getByRole('dialog', { name: 'Untrusted Project', exact: true });
}

function composer(page: Page): Locator {
  return page.getByRole('textbox', { name: 'Message Pi' });
}

async function trustCalls(
  page: Page,
): Promise<Window['__tauProjectTrustMock']['calls']> {
  return page.evaluate(() => window.__tauProjectTrustMock.calls);
}

async function finishGrant(page: Page, outcome: GrantOutcome): Promise<void> {
  await page.evaluate((result) => {
    window.__tauProjectTrustMock.finishGrant!(result);
  }, outcome);
}

const atlasRuntime = {
  ownerId: expect.any(String),
  runtimeId: expect.any(String),
  generation: expect.any(Number),
  sessionId: 'atlas-overview',
  sessionFile: expect.stringMatching(/\/atlas-overview\.jsonl$/),
};
const inspectLocal = {
  command: 'inspect_project_trust',
  args: { projectPath, runtimeCwd: projectPath, runtime: atlasRuntime },
};
const grantLocal = {
  command: 'grant_project_trust',
  args: {
    ...inspectLocal.args,
    expectedTarget: localTarget,
    inspectionToken: 'inspection-1',
  },
};

test('only the selected known-untrusted session shows an amber shield to the left of its title', async ({
  page,
}) => {
  await openWorkspace(page);
  const trigger = shield(page);
  const header = page.locator('.session-header');
  const title = header.getByRole('heading', { name: 'Workspace overview' });
  await expect(title).toBeVisible();
  const shieldBox = (await trigger.boundingBox())!;
  const titleBox = (await title.boundingBox())!;
  expect(shieldBox.x + shieldBox.width).toBeLessThanOrEqual(titleBox.x);

  for (const [scheme, stroke] of [
    ['light', 'rgb(138, 80, 0)'],
    ['dark', 'rgb(255, 188, 85)'],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await title.hover();
    await expect(trigger.locator('svg')).toHaveCSS('stroke', stroke);
    await trigger.hover();
    await expect(trigger.locator('svg')).toHaveCSS('stroke', stroke);
  }

  await title.getByRole('button').click();
  const rename = header.getByRole('textbox', { name: 'Session Name' });
  await expect(rename).toBeFocused();
  await expect(trigger).toBeVisible();
  await rename.press('Escape');
  await expect(title).toBeVisible();

  // Both sessions must be live before asserting absence: a missing report
  // and an explicit true report must not inherit Atlas overview's false.
  for (const name of ['Navigation review', 'Release checklist']) {
    await page.getByRole('button', { name: new RegExp(`^${name}`) }).click();
    await expect(
      header.getByRole('heading', { name, exact: true }),
    ).toBeVisible();
    await composer(page).fill(`Draft for ${name}`);
    await expect(
      page.getByRole('button', { name: 'Send Message' }),
    ).toBeEnabled();
    await expect(trigger).toHaveCount(0);
  }
  await page.getByRole('button', { name: /^Workspace overview/ }).click();
  await expect(
    header.getByRole('heading', { name: 'Workspace overview' }),
  ).toBeVisible();
  await expect(trigger).toBeVisible();
  expect(await trustCalls(page)).toEqual([]);
});

test('SSH confirmation identifies the long path and port inline; keyboard Escape and Cancel never write', async ({
  page,
}) => {
  await openWorkspace(page, true);
  const trigger = shield(page);
  const dialog = trustDialog(page);
  const cancel = dialog.getByRole('button', { name: 'Cancel', exact: true });
  const description = `Trusting ${sshTarget.folder} on ${sshTarget.destination} allows Pi to load project skills and run project extensions.`;
  const draft = 'Keep the SSH session draft';
  await composer(page).fill(draft);

  for (const dismissal of ['Escape', 'Cancel']) {
    await trigger.focus();
    await page.keyboard.press('Enter');
    await expect(dialog).toHaveAccessibleDescription(description);
    await expect(dialog).toHaveAttribute('data-modal-size', 'compact');
    await expect(dialog).toHaveText(
      `Untrusted Project${description}CancelTrust`,
    );
    await expect(cancel).toBeFocused();
    if (dismissal === 'Escape') await page.keyboard.press('Escape');
    else await cancel.click();
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(composer(page)).toHaveValue(draft);
  }
  const inspect = {
    command: 'inspect_project_trust',
    args: { projectPath, runtimeCwd: sshTarget.folder, runtime: atlasRuntime },
  };
  expect(await trustCalls(page)).toEqual([inspect, inspect]);
});

test('a pending grant submits once, then closes and leaves the live shield and draft intact', async ({
  page,
}) => {
  await openWorkspace(page);
  const draft = 'Keep this unsent local draft';
  await composer(page).fill(draft);
  const dialog = trustDialog(page);
  await shield(page).click();
  await expect(dialog).toHaveAccessibleDescription(
    `Trusting ${projectPath} allows Pi to load project skills and run project extensions.`,
  );
  const trust = dialog.getByRole('button', { name: 'Trust', exact: true });
  await trust.click();
  const saving = dialog.getByRole('button', { name: 'Saving…', exact: true });
  await expect(saving).toBeDisabled();
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  // Send another pointer activation without Playwright waiting for enabled.
  const box = (await saving.boundingBox())!;
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  expect(await trustCalls(page)).toEqual([inspectLocal, grantLocal]);

  await finishGrant(page, 'saved');
  await expect(dialog).toBeHidden();
  await expect(shield(page)).toBeFocused();
  await expect(composer(page)).toHaveValue(draft);

  await shield(page).press('Space');
  const close = dialog.getByRole('button', { name: 'Close', exact: true });
  await expect(dialog).toHaveAccessibleDescription(
    `Trust is already saved for ${projectPath}. This session is still untrusted. Pi checks trust again in a new process.`,
  );
  await expect(dialog.getByRole('button')).toHaveCount(1);
  await expect(close).toBeFocused();
  await close.click();
  await expect(dialog).toBeHidden();
  await expect(shield(page)).toBeFocused();
  await expect(composer(page)).toHaveValue(draft);
  expect(await trustCalls(page)).toEqual([
    inspectLocal,
    grantLocal,
    inspectLocal,
  ]);
  expect(
    await page.evaluate(() => window.__tauProjectTrustMock.runtimeChanges),
  ).toEqual(['start_pi']);
});

for (const saved of [false, true]) {
  test(`an unconfirmed grant rechecks without writing when the decision is ${saved ? 'saved' : 'not saved'}`, async ({
    page,
  }) => {
    await openWorkspace(page);
    const draft = 'Keep my draft on a lost response';
    await composer(page).fill(draft);
    await shield(page).click();
    const dialog = trustDialog(page);
    const trust = dialog.getByRole('button', { name: 'Trust', exact: true });
    await trust.click();
    await finishGrant(page, saved ? 'saved-without-ack' : 'unconfirmed');
    await expect(dialog).toHaveAccessibleDescription(
      /couldn’t confirm whether trust was saved/,
    );
    await expect(trust).toHaveCount(0);
    await dialog.getByRole('button', { name: 'Recheck', exact: true }).click();
    if (saved) {
      await expect(dialog).toHaveAccessibleDescription(
        /Trust is already saved/,
      );
      await expect(dialog.getByRole('button')).toHaveCount(1);
    } else {
      await expect(trust).toBeEnabled();
    }
    await dialog
      .getByRole('button', { name: saved ? 'Close' : 'Cancel', exact: true })
      .click();
    await expect(shield(page)).toBeFocused();
    await expect(composer(page)).toHaveValue(draft);
    expect(await trustCalls(page)).toEqual([
      inspectLocal,
      grantLocal,
      inspectLocal,
    ]);
  });
}
