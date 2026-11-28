import type { Locator, Page } from '@playwright/test';

import {
  manualRemoteConnection,
  manualRemoteDirectory,
  targetProjectPath,
  targetRemoteChild,
  targetRemoteConnection,
  targetRemoteDirectory,
  targetRemoteLabel,
} from '../support/pi-scenario/project-opening';

import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

/* The panel is portaled next to the overlay rather than inside it, so it
 * cannot inherit the overlay's centering: a regression drops it into normal
 * flow at the top-left of the page. */

const viewport = { width: 900, height: 600 };
const recentListName = 'Recent SSH destinations';
const remoteOpeningUrl =
  '/?test-scenario=project-opening&project-opening=remote';

async function openRemoteDialog(
  page: Page,
): Promise<{ dialog: Locator; input: Locator; list: Locator }> {
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();
  const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
  const input = dialog.getByRole('textbox', { name: 'SSH Connection String' });
  await expect(input).toBeFocused();
  return {
    dialog,
    input,
    list: dialog.getByRole('list', { name: recentListName }),
  };
}

function nativeCounts(page: Page, ...commands: string[]): Promise<number[]> {
  return page.evaluate(
    (names) =>
      names.map((name) =>
        window.__TAU_PI_SCENARIO__!.nativeInvocationCount(name),
      ),
    commands,
  );
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(viewport);
});

for (const step of ['connection'] as const) {
  test(`anchors the ${step} panel top-center, clear of the title bar`, async ({
    page,
  }) => {
    await page.goto(`/?fixture=remote-dialog&step=${step}`);

    const panel = page.getByRole('dialog');
    await expect(panel).toBeVisible();

    const box = await panel.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;

    // The native title bar occupies the first 30px; stay in the upper third.
    expect(box.y).toBeGreaterThanOrEqual(30);
    expect(box.y).toBeLessThan(viewport.height / 3);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
    expect(box.x + box.width / 2).toBeCloseTo(viewport.width / 2, 0);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  });
}

for (const step of ['connection'] as const) {
  test(`renders the ${step} error below its input`, async ({ page }) => {
    await page.goto(`/?fixture=remote-dialog&step=${step}&error=true`);

    const input = page.getByRole('textbox');
    const error = page.getByRole('alert');
    await expect(error).toHaveText(
      'The remote connection failed. Check the connection and try again.',
    );
    const errorId = await error.getAttribute('id');
    expect(errorId).not.toBeNull();
    await expect(input).toHaveAttribute('aria-describedby', errorId ?? '');
    expect(await input.getAttribute('title')).toBeNull();

    const inputBox = await input.boundingBox();
    const errorBox = await error.boundingBox();
    expect(inputBox).not.toBeNull();
    expect(errorBox).not.toBeNull();
    if (inputBox && errorBox) {
      expect(errorBox.y).toBeGreaterThanOrEqual(inputBox.y + inputBox.height);
    }
  });
}

test('remote directories remain keyboard and wheel reachable at native minimum size', async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 520 });
  await page.goto('/?fixture=remote-dialog&step=directory');

  const dialog = page.getByRole('dialog');
  const filter = dialog.getByRole('textbox', {
    name: 'Filter Remote Directories',
  });
  const options = dialog.getByRole('option');
  await expect(filter).toBeFocused();
  await expect(options).toHaveCount(24);
  for (let index = 1; index < 24; index += 1) {
    await filter.press('ArrowDown');
  }
  await expect(options.last()).toHaveAttribute('aria-selected', 'true');
  await expect(options.last()).toBeInViewport({ ratio: 1 });
  await expect(filter).toBeFocused();
  await expect(filter).toBeInViewport({ ratio: 1 });

  const list = dialog.getByRole('listbox');
  const box = await list.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.wheel(0, -10000);
  await expect(options.first()).toBeInViewport({ ratio: 1 });
  await expect(options.last()).not.toBeInViewport();
  await expect(filter).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('cannot be dismissed while a connection is pending', async ({ page }) => {
  await page.goto('/?fixture=remote-dialog&connecting=true');

  const panel = page.getByRole('dialog');
  await expect(panel).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(panel).toBeVisible();
  await page.locator('.ui-dialog-overlay').click({ position: { x: 5, y: 5 } });
  await expect(panel).toBeVisible();
});

for (const mode of ['local', 'remote'] as const) {
  test(`opens an existing ${mode} project without losing the source draft`, async ({
    page,
  }) => {
    await page.goto(`/?test-scenario=project-opening&project-opening=${mode}`);

    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await expect(composer).toBeEnabled();
    await composer.fill('Keep the source draft');

    if (mode === 'local') {
      await page.getByRole('button', { name: 'Open Project' }).click();
      await page.getByRole('menuitem', { name: 'Open Local Project' }).click();
    } else {
      const { input } = await openRemoteDialog(page);
      await input.fill(targetRemoteConnection);
      await input.press('Meta+Enter');
      await page
        .getByRole('option', {
          name: `Select ${targetRemoteDirectory}`,
          exact: true,
        })
        .click();
    }

    const targetSession = page.locator('[data-session-id="session-target"]');
    await expect(targetSession).toHaveClass(/selected/);
    await expect(composer).toBeEnabled();
    await expect(composer).toHaveValue('');
    await composer.fill('Target composer is usable');

    await page
      .locator('[data-session-id="session-main"] .session-select')
      .click();
    await expect(composer).toHaveValue('Keep the source draft');
  });
}

test('remembers only completed imports, replays the explicit port, and keeps history after project removal', async ({
  page,
  browserName,
}) => {
  await page.goto(remoteOpeningUrl);
  const source = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(source).toBeEnabled();
  const trigger = page.getByRole('button', { name: 'Open Project' });
  const directory = page.getByRole('dialog', {
    name: 'Choose Remote Working Directory',
  });
  const target = page.locator(`[data-project-path="${targetProjectPath}"]`);
  const selectRoot = directory.getByRole('option', {
    name: `Select ${targetRemoteDirectory}`,
    exact: true,
  });

  const connection = await openRemoteDialog(page);
  await expect(connection.input).toHaveValue('');
  await expect(connection.list).toHaveCount(0);
  await connection.input.fill(targetRemoteConnection);
  await page.keyboard.press('Enter');
  await directory.getByRole('option', { name: 'child' }).click();
  await expect(
    directory.getByRole('option', { name: `Select ${targetRemoteChild}` }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(directory).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(
    await nativeCounts(
      page,
      'probe_remote_project',
      'list_remote_directories',
      'import_remote_project',
    ),
  ).toEqual([1, 1, 0]);

  await openRemoteDialog(page);
  await expect(connection.list).toHaveCount(0);
  await connection.input.fill(targetRemoteConnection);
  await page.keyboard.press('Enter');
  await selectRoot.click();
  await expect(directory).toHaveCount(0);
  await expect(
    target.locator('[data-session-id="session-target"]'),
  ).toHaveClass(/selected/);
  await expect(source).toBeEnabled();

  await openRemoteDialog(page);
  await expect(connection.input).toHaveValue('');
  const destination = connection.list.getByRole('button', {
    name: targetRemoteLabel,
    exact: true,
  });
  await page.keyboard.press('ArrowDown');
  await expect(destination).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(connection.input).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(destination).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(selectRoot).toBeVisible();
  await expect(page.getByRole('list', { name: recentListName })).toHaveCount(0);
  expect(
    await nativeCounts(
      page,
      'probe_remote_project',
      'import_remote_project',
      'start_pi_remote',
    ),
  ).toEqual([3, 1, 1]);
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();

  await openRemoteDialog(page);
  await expect(destination).toBeVisible();
  await connection.input.fill(manualRemoteConnection);
  await page.keyboard.press('Enter');
  await expect(
    directory.getByRole('option', { name: `Select ${manualRemoteDirectory}` }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(directory).toHaveCount(0);

  await target
    .getByRole('button', { name: 'Target fixture', exact: true })
    .click({ button: 'right' });
  await page
    .getByRole('menuitem', { name: 'Remove Project', exact: true })
    .click();
  const removal = page.getByRole('alertdialog');
  await removal.getByRole('button', { name: 'Remove', exact: true }).click();
  await expect(removal).toHaveCount(0);
  await expect(target).toHaveCount(0);

  await openRemoteDialog(page);
  await expect(destination).toBeVisible();
  await page.keyboard.press('ArrowDown');
  await expect(destination).toBeFocused();
  // macOS WebKit's Option+Tab includes buttons in sequential focus navigation.
  await page.keyboard.press(
    browserName === 'webkit' && process.platform === 'darwin'
      ? 'Alt+Tab'
      : 'Tab',
  );
  const forget = connection.list.getByRole('button', {
    name: `Forget ${targetRemoteLabel}`,
    exact: true,
  });
  await expect(forget).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(connection.list).toHaveCount(0);
  await expect(connection.input).toBeFocused();
  expect(
    await nativeCounts(
      page,
      'probe_remote_project',
      'import_remote_project',
      'remove_project',
      'forget_ssh_destination',
    ),
  ).toEqual([4, 1, 1, 1]);

  await page.keyboard.press('Escape');
  await openRemoteDialog(page);
  await expect(connection.list).toHaveCount(0);
});

test('a failed pointer Forget keeps the row and typed input without connecting or removing projects', async ({
  page,
}) => {
  await page.goto(
    '/?test-scenario=saved-session-bootstrap&recent-ssh=forget-failure',
  );
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const { dialog, input, list } = await openRemoteDialog(page);
  const target = list.getByRole('button', {
    name: targetRemoteLabel,
    exact: true,
  });
  const forget = list.getByRole('button', {
    name: `Forget ${targetRemoteLabel}`,
    exact: true,
  });
  await input.fill('ssh typed-alias');
  await forget.click();
  await expect(dialog.getByRole('alert')).toContainText('Could not forget');
  await expect(target).toBeVisible();
  await expect(input).toHaveValue('ssh typed-alias');
  await forget.click();
  await expect(target).toHaveCount(0);
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  await expect(input).toHaveValue('ssh typed-alias');
  await expect(
    list.getByRole('button', { name: 'build.example', exact: true }),
  ).toBeVisible();
  expect(
    await nativeCounts(
      page,
      'forget_ssh_destination',
      'probe_remote_project',
      'import_remote_project',
      'remove_project',
      'stop_pi',
    ),
  ).toEqual([2, 0, 0, 0, 0]);
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('button', { name: 'Tau fixture', exact: true }),
  ).toBeVisible();
});

test('recent destinations with long labels remain keyboard and wheel reachable at native minimum size', async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 520 });
  await page.goto(
    '/?test-scenario=saved-session-bootstrap&recent-ssh=overflow',
  );
  await expect(page.getByRole('textbox', { name: 'Message Pi' })).toBeEnabled();
  const { dialog, input, list } = await openRemoteDialog(page);
  const longDestination = list.getByRole('button', { name: /^long-alias-/ });
  const forget = list.getByRole('button', { name: /^Forget long-alias-/ });
  const last = list.getByRole('button', {
    name: 'build-23.example',
    exact: true,
  });
  await expect(list.getByRole('listitem')).toHaveCount(24);
  await expect(last).not.toBeInViewport();
  await expect(forget).toBeInViewport({ ratio: 1 });
  const panel = (await dialog.boundingBox())!;
  const long = (await longDestination.boundingBox())!;
  const remove = (await forget.boundingBox())!;
  expect(panel.y + panel.height).toBeLessThanOrEqual(520);
  expect(long.x + long.width).toBeLessThanOrEqual(remove.x);
  expect(remove.x + remove.width).toBeLessThanOrEqual(panel.x + panel.width);

  for (let index = 0; index < 24; index += 1)
    await page.keyboard.press('ArrowDown');
  await expect(last).toBeFocused();
  await expect(last).toBeInViewport({ ratio: 1 });
  await expect(input).toBeInViewport({ ratio: 1 });

  const box = (await list.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -10_000);
  await expect(longDestination).toBeInViewport({ ratio: 1 });
  await expect(last).not.toBeInViewport();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('uses the existing read-only connection without recents in retry mode', async ({
  page,
}) => {
  await page.goto(
    '/?test-scenario=remote-phantom-prompt-process-exit&recent-ssh=seeded',
  );
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  const { dialog, input, list } = await openRemoteDialog(page);
  await expect(
    list.getByRole('button', { name: targetRemoteLabel, exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);

  await page.getByRole('button', { name: 'New Session', exact: true }).click();
  await composer.fill('Retry this remote session');
  await page.getByRole('button', { name: 'Send Message' }).click();
  for (const gate of [
    'before-remote-phantom-prompt-process-failure',
    'after-remote-phantom-prompt-bridge-error',
    'after-remote-phantom-prompt-process-exit',
  ]) {
    await waitForGate(page, gate);
    await releaseGate(page, gate);
  }
  await expect(dialog).toBeVisible();
  await expect(input).toHaveValue('ssh fixture@example');
  await expect(input).toHaveAttribute('readonly', '');
  await expect(list).toHaveCount(0);
  await input.press('Enter');
  await expect(dialog).toHaveCount(0);
  await expect(composer).toBeEnabled();
  expect(
    await nativeCounts(page, 'probe_remote_project', 'import_remote_project'),
  ).toEqual([0, 0]);
  await openRemoteDialog(page);
  await expect(list.getByRole('listitem')).toHaveCount(2);
});

test('restores focus to the open-project trigger after dismissal', async ({
  page,
}) => {
  await page.goto('/?test-scenario=saved-session-bootstrap');

  const trigger = page.getByRole('button', { name: 'Open Project' });
  await trigger.hover();
  const tooltip = page.locator('.ui-tooltip:visible');
  await expect(tooltip).toHaveCount(1);
  await expect(tooltip.locator('.ui-tooltip-label')).toHaveText('Open Project');
  await expect(tooltip.locator('kbd')).toHaveText(/^(⌘|Ctrl\+)O$/);
  const anchor = (await trigger.boundingBox())!;
  await trigger.click();
  await expect(tooltip).toHaveCount(0);
  const menu = page.getByRole('menu');
  await expect(menu).toBeVisible();
  const bounds = (await menu.boundingBox())!;
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(anchor.y);
  expect(Math.abs(bounds.x - anchor.x)).toBeLessThan(12);
  await page.getByRole('menuitem', { name: 'Open Remote Project' }).click();

  const dialog = page.getByRole('dialog', { name: 'SSH Connection' });
  await expect(dialog).toBeVisible();
  await page.keyboard.press('Escape');

  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
});
