import type { Page } from '@playwright/test';

import {
  blockedCompactionCases,
  clickedMessage,
  compactionQueueRaceCases,
  compactionQueueNativeProfile,
  exactDraft,
  followUpMessage,
  initialPrompt,
  steeringMessage,
} from '../support/pi-scenario/saved-session-compaction-queue';

import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

test.use({ pausedClock: true });

const restriction =
  'Sending is unavailable during this compaction. Send your draft after it finishes.';

async function followUpShortcut(page: Page): Promise<string> {
  return (await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform)))
    ? 'Meta+Enter'
    : 'Control+Enter';
}

async function openCompaction(
  page: Page,
  name: string,
  active = true,
): Promise<void> {
  const profile = compactionQueueNativeProfile(
    `saved-session-compaction-queue-${name}`,
  );
  // Exercise startup without relying on page load or composer actions for readiness.
  await page.goto(
    `/?test-scenario=${profile}&test-scenario-variant=saved-session-compaction-queue-${name}`,
    { waitUntil: 'commit' },
  );
  await page.waitForFunction(() => Boolean(window.__TAU_PI_SCENARIO__));
  if (active) {
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await composer.fill(initialPrompt);
    await composer.press('Enter');
  }
  await waitForGate(page, 'compaction-ready');
  await expect(page.locator('.transient-compaction')).toHaveText('compacting');
}

for (const reason of ['threshold', 'manual']) {
  test(`isolates pre-run ${reason} compaction and exact drafts across session switching`, async ({
    page,
  }) => {
    await openCompaction(page, `navigation-${reason}`, false);
    await releaseGate(page, 'compaction-ready');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    const queue = page.getByRole('region', { name: 'Pending messages' });
    await composer.fill(exactDraft);
    await page.keyboard.press('Control+Tab');
    await waitForGate(page, 'backup-ready');
    await expect(page.getByRole('heading', { name: 'Backup' })).toBeVisible();
    await expect(composer).toHaveValue('');
    await expect(page.locator('.transient-compaction')).toHaveCount(0);
    await expect(page.getByText(restriction, { exact: true })).toHaveCount(0);
    await expect(queue).toHaveCount(0);
    await composer.fill('Backup draft stays separate');
    await expect(
      page.getByRole('button', { name: 'Send Message', exact: true }),
    ).toBeEnabled();

    await page.keyboard.press('Control+Shift+Tab');
    await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
    await expect(composer).toHaveValue(exactDraft);
    await expect(page.locator('.transient-compaction')).toHaveText(
      'compacting',
    );
    await expect(
      page.getByRole('button', { name: 'Send Message', exact: true }),
    ).toBeDisabled();
    await expect(page.getByText(restriction, { exact: true })).toBeVisible();
    await composer.press('Enter');
    await expect(composer).toHaveValue(exactDraft);
    await expect(queue).toHaveCount(0);
    await releaseGate(page, 'backup-ready');
    await expect
      .poll(() =>
        page.evaluate(() =>
          window.__TAU_PI_SCENARIO__?.nativeInvocationCount('start_pi'),
        ),
      )
      .toBe(2);
    expect(
      await page.evaluate(() =>
        window.__TAU_PI_SCENARIO__?.nativeInvocationCount('stop_pi'),
      ),
    ).toBe(0);
  });
}

for (const reason of ['threshold', 'overflow']) {
  test(`queues Enter, click steering, and modifier follow-up during active ${reason} compaction`, async ({
    page,
  }) => {
    await openCompaction(page, reason);
    await releaseGate(page, 'compaction-ready');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    const queue = page.getByRole('region', { name: 'Pending messages' });
    await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeDisabled();
    await composer.fill(steeringMessage);
    await expect(page.getByText(restriction)).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Queue Message', exact: true }),
    ).toBeEnabled();
    await composer.press('Enter');
    await waitForGate(page, 'steer-visible');
    await expect(composer).toHaveValue('');
    await expect(
      queue.getByRole('button', {
        name: `Steering: ${steeringMessage}`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.locator('.message.user').filter({ hasText: steeringMessage }),
    ).toHaveCount(0);
    await releaseGate(page, 'steer-visible');

    await composer.fill(clickedMessage);
    await page
      .getByRole('button', { name: 'Queue Message', exact: true })
      .click();
    await waitForGate(page, 'clicked-visible');
    await expect(composer).toHaveValue('');
    await expect(
      queue.getByRole('button', {
        name: `Steering: ${clickedMessage}`,
        exact: true,
      }),
    ).toBeVisible();
    await releaseGate(page, 'clicked-visible');

    await composer.fill(followUpMessage);
    await composer.press(await followUpShortcut(page));
    await waitForGate(page, 'follow-up-visible');
    await expect(composer).toHaveValue('');
    await expect(
      queue.getByRole('button', {
        name: `Follow Up 1: ${followUpMessage}`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.locator('.message.user').filter({ hasText: followUpMessage }),
    ).toHaveCount(0);
    await expect(page.locator('.transient-compaction')).toHaveText(
      'compacting',
    );
    await expect(page.getByRole('button', { name: 'Stop Pi' })).toBeDisabled();
    await releaseGate(page, 'follow-up-visible');
  });
}

for (const { key, active } of blockedCompactionCases) {
  test(`keeps an editable draft and denies queue admission during ${key} compaction`, async ({
    page,
  }) => {
    await openCompaction(page, `blocked-${key}`, active);
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await expect(composer).toBeEditable();
    await composer.fill(exactDraft);
    await expect(page.getByText(restriction, { exact: true })).toBeVisible();
    await expect(
      page.getByRole('button', {
        name: active ? 'Queue Message' : 'Send Message',
        exact: true,
      }),
    ).toBeDisabled();
    await composer.press('Enter');
    await expect(composer).toHaveValue(exactDraft);
    await composer.press(await followUpShortcut(page));
    await expect(composer).toHaveValue(exactDraft);
    await expect(
      page.getByRole('region', { name: 'Pending messages' }),
    ).toHaveCount(0);
    await releaseGate(page, 'compaction-ready');
    await waitForGate(page, 'compaction-finished');
    await expect(page.getByText(restriction, { exact: true })).toHaveCount(0);
    await expect(composer).toHaveValue(exactDraft);
    await expect(
      page.getByRole('button', {
        name: active ? 'Queue Message' : 'Send Message',
        exact: true,
      }),
    ).toBeEnabled();
    await expect(
      page.getByRole('region', { name: 'Pending messages' }),
    ).toHaveCount(0);
    await releaseGate(page, 'compaction-finished');
  });
}

for (const race of compactionQueueRaceCases) {
  test(`recovers the exact provisional draft without replay when ${race} overtakes queue mode setup`, async ({
    page,
  }) => {
    await openCompaction(page, `race-${race}`);
    await releaseGate(page, 'compaction-ready');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await composer.fill(exactDraft);
    await expect(
      page.getByRole('button', { name: 'Queue Message', exact: true }),
    ).toBeEnabled();
    await composer.press(
      race === 'settlement' ? await followUpShortcut(page) : 'Enter',
    );
    await waitForGate(page, 'queue-mode-pending');
    await expect(composer).toHaveValue('');
    await releaseGate(page, 'queue-mode-pending');
    await waitForGate(page, 'queue-mode-resolved');
    await expect(composer).toHaveValue(exactDraft);
    await expect(
      page.locator('.queue-feedback').filter({ hasText: 'Submitting…' }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: /^(Steering:|Follow Up \d+:)/ }),
    ).toHaveCount(0);
    await releaseGate(page, 'queue-mode-resolved');
    await waitForGate(page, 'admission-restored');
    await expect(composer).toHaveValue(exactDraft);
    await expect(
      page.getByRole('button', {
        name: race === 'settlement' ? 'Send Message' : 'Queue Message',
        exact: true,
      }),
    ).toBeEnabled();
    await expect(
      page.locator('.message.user').filter({ hasText: exactDraft.trim() }),
    ).toHaveCount(0);
    await releaseGate(page, 'admission-restored');
  });
}
