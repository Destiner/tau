import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const warning = 'Keep this warning between the two segments';
const backgroundWarning = 'Only Main owns this background warning';
const gapRows = [
  'Before the notice.',
  `Warning${warning}`,
  'After the notice.',
];

test.use({ pausedClock: true });

async function assertGap(page: Page): Promise<void> {
  await expect(
    page.locator('.message.notice').filter({ hasText: warning }),
  ).toHaveCount(1);
  await expect
    .poll(() =>
      page
        .locator('.message-window > .message')
        .evaluateAll((rows) =>
          rows
            .map((row) => row.textContent?.trim() ?? '')
            .filter(
              (text) =>
                text.includes('Before the notice.') ||
                text.includes('After the notice.') ||
                text.includes('Keep this warning between the two segments'),
            ),
        ),
    )
    .toEqual(gapRows);
}

async function gapIds(page: Page): Promise<(string | null)[]> {
  return page
    .locator('.message-window > .message')
    .evaluateAll((rows) =>
      rows
        .filter((row) =>
          /Before the notice\.|After the notice\.|Keep this warning between the two segments/.test(
            row.textContent ?? '',
          ),
        )
        .map((row) => row.getAttribute('data-message-id')),
    );
}

async function switchSession(page: Page, name: string): Promise<void> {
  const modifier = (await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(navigator.platform),
  ))
    ? 'Meta'
    : 'Control';
  await page.keyboard.press(`${modifier}+p`);
  await page
    .getByRole('dialog', { name: 'Switch Session' })
    .getByRole('option', { name })
    .click();
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
}

async function positionReader(page: Page, reading: boolean): Promise<number> {
  if (reading) {
    await page.getByLabel('Transcript', { exact: true }).evaluate((element) => {
      element.scrollTop = 240;
      element.dispatchEvent(new Event('scroll'));
    });
  } else {
    await page.getByLabel('Transcript', { exact: true }).evaluate((element) => {
      element.scrollTop = element.scrollHeight;
      element.dispatchEvent(new Event('scroll'));
    });
  }
  await page.clock.runFor(100);
  if (reading) {
    await expect(
      page.getByText('Field note 8: The synthetic garden is quiet.', {
        exact: true,
      }),
    ).toBeInViewport();
  }
  return readerTop(page);
}

async function readerTop(page: Page): Promise<number> {
  const box = await page
    .getByText('Field note 8: The synthetic garden is quiet.', { exact: true })
    .boundingBox();
  expect(box).not.toBeNull();
  return box!.y;
}

async function assertViewport(
  page: Page,
  reading: boolean,
  top: number,
): Promise<void> {
  await page.clock.runFor(100);
  if (reading) {
    await expect
      .poll(async () => Math.abs((await readerTop(page)) - top))
      .toBeLessThan(3);
  } else {
    await expect
      .poll(() =>
        page
          .getByLabel('Transcript', { exact: true })
          .evaluate(
            (element) =>
              element.scrollHeight - element.scrollTop - element.clientHeight,
          ),
      )
      .toBeLessThanOrEqual(3);
  }
}

for (const reading of [false, true]) {
  test(`keeps notice boundaries and warm session isolation while ${reading ? 'reading history' : 'following the end'}`, async ({
    page,
  }) => {
    await page.goto('/?test-scenario=saved-session-notice-boundaries');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await composer.fill('Inspect the boundary');
    await composer.press('Enter');
    await waitForGate(page, 'live-gap');
    await page.clock.runFor(100);
    await assertGap(page);
    const ids = await gapIds(page);
    expect(ids.every(Boolean)).toBe(true);
    expect(new Set(ids).size).toBe(3);
    let top = await positionReader(page, reading);

    await releaseGate(page, 'live-gap');
    for (const hydration of ['first-hydration', 'repeated-hydration']) {
      await waitForGate(page, `${hydration}-pending`);
      await releaseGate(page, `${hydration}-pending`);
      await waitForGate(page, `${hydration}-complete`);
      await assertGap(page);
      expect(await gapIds(page)).toEqual(ids);
      await assertViewport(page, reading, top);
      await releaseGate(page, `${hydration}-complete`);
    }

    await page.clock.runFor(1);
    await composer.fill('Continue the inspection');
    await composer.press('Enter');
    await waitForGate(page, 'later-response-pending');
    await composer.fill('Main draft survives warm navigation');
    top = await positionReader(page, reading);
    await releaseGate(page, 'later-response-pending');
    await switchSession(page, 'Backup');
    await waitForGate(page, 'backup-ready');
    await expect(composer).toHaveValue('');
    await composer.fill('Independent Backup draft');
    await expect(
      page.getByText('Backup session stays independent.', { exact: true }),
    ).toBeVisible();
    await releaseGate(page, 'backup-ready');
    await waitForGate(page, 'later-hydration-pending');
    await releaseGate(page, 'later-hydration-pending');
    await waitForGate(page, 'later-hydration-complete');
    await expect(page.locator('.message.notice')).toHaveCount(0);
    await expect(
      page.getByText('A later response arrived in the background.', {
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(composer).toHaveValue('Independent Backup draft');
    await releaseGate(page, 'later-hydration-complete');
    await switchSession(page, 'Main');
    await waitForGate(page, 'returned-to-main');
    await expect(composer).toHaveValue('Main draft survives warm navigation');
    await assertGap(page);
    expect(await gapIds(page)).toEqual(ids);
    await expect(
      page.locator('.message.notice').filter({ hasText: backgroundWarning }),
    ).toHaveCount(1);
    await expect(
      page.getByText('Backup session stays independent.', { exact: true }),
    ).toHaveCount(0);
    await expect
      .poll(() =>
        page
          .locator('.message-window > .message')
          .evaluateAll((rows) =>
            rows.slice(-6).map((row) => row.textContent?.trim() ?? ''),
          ),
      )
      .toEqual([
        ...gapRows,
        'Continue the inspection',
        'A later response arrived in the background.',
        `Warning${backgroundWarning}`,
      ]);
    await assertViewport(page, reading, top);
    const allIds = await page
      .locator('[data-message-id]')
      .evaluateAll((rows) =>
        rows.map((row) => row.getAttribute('data-message-id')),
      );
    expect(new Set(allIds).size).toBe(allIds.length);
    await releaseGate(page, 'returned-to-main');
  });
}
