import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

test.use({ pausedClock: true });

async function requests(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    window
      .__TAU_PI_SCENARIO__!.timeline()
      .flatMap((entry) =>
        entry.kind === 'request' ? [entry.request.type] : [],
      ),
  );
}

for (const recovery of ['retry', 'picker'] as const) {
  test(`initial model rejection preserves skill drafts until deliberate ${recovery} recovery`, async ({
    page,
  }) => {
    const variant =
      recovery === 'picker'
        ? 'initial-settings-failure-picker'
        : 'initial-settings-failure';
    await page.goto(
      `/?test-scenario=phantom-command-only&test-scenario-variant=${variant}`,
    );
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    const send = page.getByRole('button', { name: 'Send Message' });
    const retry = page.getByRole('button', {
      name: 'Retry Setup',
      exact: true,
    });
    const notice = page
      .getByRole('alert')
      .filter({ hasText: 'Model setup failed' });
    await expect(composer).toBeEnabled();
    await page
      .getByRole('button', { name: 'New Session', exact: true })
      .click();
    await composer.fill('/synt');
    await expect(
      page.locator('.command-option[aria-selected="true"]'),
    ).toContainText('/skill:synthetic');
    await composer.press('Enter');
    await waitForGate(page, 'setup-blocked');

    await expect(notice).toBeVisible();
    await expect(retry).toBeEnabled();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(
      page.getByText('PRIVATE_SETUP_CANARY', { exact: false }),
    ).toHaveCount(0);
    await expect(composer).toBeEnabled();
    await expect(composer).toBeFocused();
    await expect(composer).toHaveValue('/skill:synthetic');
    const noticeBounds = await notice.boundingBox();
    const composerBounds = await composer.boundingBox();
    expect(noticeBounds!.y + noticeBounds!.height).toBeLessThanOrEqual(
      composerBounds!.y,
    );
    await expect(
      page.getByRole('combobox', {
        name: 'Thinking Effort',
        exact: true,
      }),
    ).toBeEnabled();
    await expect(send).toBeDisabled();
    const blockedRequests = await requests(page);
    expect(blockedRequests.slice(-3)).toEqual([
      'set_model',
      'get_state',
      'get_available_thinking_levels',
    ]);
    expect(blockedRequests).not.toContain('prompt');

    await send.evaluate((element) => (element as HTMLButtonElement).click());
    for (const key of ['Enter', 'Meta+Enter', 'Control+Enter']) {
      await composer.press(key);
      await expect(composer).toHaveValue('/skill:synthetic');
    }
    await composer.fill('/synt');
    await composer.press('Enter');
    // A blocked completion must not consume the draft or schedule a future send.
    await expect(composer).toHaveValue(/^\/(synt|skill:synthetic)$/);
    await composer.fill('Keep this ordinary draft blocked');
    for (const key of ['Enter', 'Meta+Enter', 'Control+Enter']) {
      await composer.press(key);
      await expect(composer).toHaveValue('Keep this ordinary draft blocked');
    }
    await composer.fill('/skill:synthetic edited before recovery');
    await expect(page.locator('.message.user')).toHaveCount(0);
    expect(await requests(page)).toEqual(blockedRequests);

    await releaseGate(page, 'setup-blocked');
    if (recovery === 'retry') {
      await retry.click();
    } else {
      await page.getByRole('combobox', { name: 'Model', exact: true }).click();
      const fallbackModel = page.getByRole('option', {
        name: 'Beta',
        exact: true,
      });
      await expect(fallbackModel).toHaveAttribute('aria-selected', 'true');
      await fallbackModel.click();
    }
    await waitForGate(page, 'setup-recovering');
    await expect(send).toBeDisabled();
    await composer.fill('/skill:synthetic edited during recovery');
    await composer.press('Enter');
    await expect(composer).toHaveValue(
      '/skill:synthetic edited during recovery',
    );
    await releaseGate(page, 'setup-recovering');
    await waitForGate(page, 'setup-ready');
    await expect(notice).toHaveCount(0);
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(composer).toHaveValue(
      '/skill:synthetic edited during recovery',
    );
    await expect(send).toBeEnabled();
    expect(
      (await requests(page)).filter((type) => type === 'prompt'),
    ).toHaveLength(0);
    expect((await requests(page)).slice(blockedRequests.length)).toEqual(
      recovery === 'retry'
        ? [
            'set_model',
            'set_thinking_level',
            'get_state',
            'get_available_thinking_levels',
          ]
        : ['set_model', 'get_state', 'get_available_thinking_levels'],
    );
    await releaseGate(page, 'setup-ready');
    await send.click();
    await expect
      .poll(
        async () =>
          (await requests(page)).filter((type) => type === 'prompt').length,
      )
      .toBe(1);
  });
}
