import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&compacted=true';

for (const scheme of ['light', 'dark'] as const) {
  test(`loads earlier compacted history without moving the divider (${scheme})`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto(fixtureUrl);
    const transcript = page.locator('.transcript');
    await transcript.evaluate((element) => {
      element.scrollTop = 0;
    });
    await expect(
      page.locator('[data-message-id="current-fixture-assistant-1"] pre.shiki'),
    ).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const button = page.getByRole('button', { name: 'Load earlier messages' });
    await expect(button).toHaveText('compacted');
    await expect(button).not.toHaveClass(/pulse/);
    await button.scrollIntoViewIfNeeded();
    await expect(button).toBeInViewport();
    const before = await button.boundingBox();
    expect(before).not.toBeNull();

    await button.click();

    const divider = page.locator(
      '[data-message-id="fixture-compaction"] .compaction-divider',
    );
    await expect(divider).toHaveText('compacted');
    await expect(page.getByTestId('fixture-count')).toHaveText('29 messages');
    await expect(
      page.getByRole('button', { name: 'Load earlier messages' }),
    ).toHaveCount(0);

    await expect
      .poll(async () => {
        const after = await divider.boundingBox();
        return after && before ? Math.abs(after.y - before.y) : Number.NaN;
      })
      .toBeLessThanOrEqual(2);
    expect(
      await transcript.evaluate((element) => element.scrollTop),
    ).toBeGreaterThan(0);
  });
}
