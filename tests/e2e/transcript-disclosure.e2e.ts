import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&compact-tools=true';
const firstMessage = '[data-message-id="fixture-compact-user"]';
const finalTool = '[data-message-id="fixture-compact-tool-23"]';

interface TranscriptGeometry {
  firstIndex: number;
  rowCount: number;
  scrollTop: number;
  underfilled: boolean;
  userVisible: boolean;
}

async function geometry(page: Page): Promise<TranscriptGeometry> {
  return await page.evaluate((userSelector) => {
    const transcript = document.querySelector(
      '[aria-label="Transcript"]',
    ) as HTMLElement | null;
    const rows = Array.from(
      transcript?.querySelectorAll<HTMLElement>('[data-index]') ?? [],
    );
    const user = document.querySelector(userSelector);
    if (!transcript) {
      return {
        firstIndex: -1,
        rowCount: rows.length,
        scrollTop: -1,
        underfilled: false,
        userVisible: false,
      };
    }
    const transcriptBox = transcript.getBoundingClientRect();
    const userBox = user?.getBoundingClientRect();
    return {
      firstIndex:
        rows.length > 0
          ? Math.min(...rows.map((row) => Number(row.dataset.index ?? -1)))
          : -1,
      rowCount: rows.length,
      scrollTop: Math.round(transcript.scrollTop),
      underfilled: transcript.scrollHeight <= transcript.clientHeight,
      userVisible: Boolean(
        userBox &&
        userBox.top >= transcriptBox.top &&
        userBox.bottom <= transcriptBox.bottom,
      ),
    };
  }, firstMessage);
}

async function expectWholeTranscript(page: Page): Promise<void> {
  await expect.poll(async () => (await geometry(page)).underfilled).toBe(true);
  await expect.poll(async () => (await geometry(page)).scrollTop).toBe(0);
  await expect.poll(async () => (await geometry(page)).firstIndex).toBe(0);
  await expect.poll(async () => (await geometry(page)).rowCount).toBe(24);
  await expect.poll(async () => (await geometry(page)).userVisible).toBe(true);
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1_280, height: 1_000 });
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('24 messages');
  await expect(page.locator(finalTool)).toBeVisible();
  await expectWholeTranscript(page);
});

test('scrolls an expanded tool call as one payload', async ({ page }) => {
  await page.goto('/?fixture=long-transcript&verbose-tool=true');

  const tool = page.locator('[data-message-id="fixture-verbose-tool"]');
  await tool.locator('.activity-header').click();

  const details = tool.locator('.activity-details');
  const argumentsBody = details.locator('.activity-detail-body').nth(0);
  const resultBody = details.locator('.activity-detail-body').nth(1);
  await expect(details).toBeVisible();
  await expect(argumentsBody).toContainText('ARGUMENT_TAIL_SENTINEL');
  await expect(resultBody).toContainText('RESULT_TAIL_SENTINEL');
  await expect.poll(() => resultBody.textContent()).not.toMatch(/\n…$/);
  await expect
    .poll(() =>
      details.evaluate((element) => ({
        clientHeight: element.clientHeight,
        overflowY: getComputedStyle(element).overflowY,
        scrollHeight: element.scrollHeight,
        nestedScrollers: Array.from(
          element.querySelectorAll<HTMLElement>('.activity-detail-body'),
        ).filter((body) => {
          const style = getComputedStyle(body);
          return (
            body.scrollHeight > body.clientHeight &&
            (style.overflowY === 'auto' || style.overflowY === 'scroll')
          );
        }).length,
      })),
    )
    .toMatchObject({ overflowY: 'auto', nestedScrollers: 0 });
  await expect
    .poll(() =>
      details.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    )
    .toBe(true);

  await details.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(() =>
      details.evaluate(
        (element) =>
          element.scrollHeight - element.clientHeight - element.scrollTop,
      ),
    )
    .toBeLessThanOrEqual(1);
  await expect
    .poll(() => resultBody.textContent())
    .toMatch(/RESULT_TAIL_SENTINEL$/);
});

test('keeps early rows mounted when an underfilled tool run is expanded', async ({
  page,
}) => {
  const header = page.locator(`${finalTool} .activity-header`);

  await header.click();
  await expect(header).toHaveAttribute('aria-expanded', 'true');
  await expectWholeTranscript(page);

  await header.click();
  await expect(header).toHaveAttribute('aria-expanded', 'false');
  await expectWholeTranscript(page);

  await header.click();
  await expect(header).toHaveAttribute('aria-expanded', 'true');
  await expectWholeTranscript(page);
});

test('does not restore a poisoned offset after switching sessions', async ({
  page,
}) => {
  const header = page.locator(`${finalTool} .activity-header`);
  await header.click();
  await expect(header).toHaveAttribute('aria-expanded', 'true');

  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.switchSession('elsewhere'),
  );
  await expect(page.getByTestId('fixture-session')).toHaveText('elsewhere');
  await page.evaluate(() =>
    window.__TAU_TRANSCRIPT_FIXTURE__?.switchSession('main'),
  );
  await expect(page.getByTestId('fixture-session')).toHaveText('main');

  await expectWholeTranscript(page);
  const restoredHeader = page.locator(`${finalTool} .activity-header`);
  await restoredHeader.click();
  await expect(restoredHeader).toHaveAttribute('aria-expanded', 'true');
  await expectWholeTranscript(page);
});
