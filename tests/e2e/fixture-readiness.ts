import type { Page } from '@playwright/test';

import { expect } from './fixtures';

export default async function waitForShowcaseRendering(
  page: Page,
): Promise<void> {
  const showcase = page.locator(
    '[data-message-id="fixture-markdown-showcase"]',
  );
  await expect(showcase.locator('.diagram')).toHaveCount(5);
  await expect(
    showcase.locator('.code-block[data-tau-lang="ts"] pre.shiki'),
  ).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}
