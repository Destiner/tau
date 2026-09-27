import type { Page } from '@playwright/test';

import appVersion from '../../src/lib/app-version';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap';

function nativeCalls(page: Page, command: string): Promise<number> {
  return page.evaluate(
    (name) => window.__TAU_PI_SCENARIO__?.nativeInvocationCount(name) ?? 0,
    command,
  );
}

test('toggles the issue reporter through the sidebar code but ignores composer input', async ({
  page,
}) => {
  await test.step('ignores the code typed into the composer', async () => {
    await page.goto(scenarioUrl);
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await expect(composer).toBeEnabled();
    await composer.click();
    await page.keyboard.type('iddqd');
    await expect(composer).toHaveValue('iddqd');
    await expect(
      page.getByRole('button', { name: 'Report an Issue' }),
    ).toHaveCount(0);
    expect(await nativeCalls(page, 'set_admin_mode')).toBe(0);
  });

  await test.step('keeps diagnostics out of an ordinary run until the code unlocks them', async () => {
    await expect(
      page.getByRole('textbox', { name: 'Message Pi' }),
    ).toBeEnabled();
    const footer = page.getByLabel('Projects and Sessions').locator('footer');
    const version = footer.getByLabel(`Tau version ${appVersion}`);
    await expect(version).toHaveText(`v${appVersion}`);
    await expect(version).toBeVisible();
    const versionStyles = await version.evaluate((element) => {
      const mutedProbe = document.createElement('span');
      mutedProbe.style.color = 'var(--muted)';
      document.body.append(mutedProbe);

      const styles = getComputedStyle(element);
      const result = {
        color: styles.color,
        fontFamily: styles.fontFamily,
        mutedColor: getComputedStyle(mutedProbe).color,
      };
      mutedProbe.remove();
      return result;
    });
    expect(versionStyles.fontFamily).toBe(
      'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
    );
    expect(versionStyles.color).toBe(versionStyles.mutedColor);
    const reportIssue = page.getByRole('button', { name: 'Report an Issue' });
    await expect(reportIssue).toHaveCount(0);
    await page.getByLabel('Projects and Sessions').click();
    await page.keyboard.type('iddqd');
    await expect(reportIssue).toBeVisible();
    const archiveBounds = await footer
      .getByRole('button', { name: 'Show Archived Sessions' })
      .boundingBox();
    const reportBounds = await reportIssue.boundingBox();
    const versionBounds = await version.boundingBox();
    expect(archiveBounds).not.toBeNull();
    expect(reportBounds).not.toBeNull();
    expect(versionBounds).not.toBeNull();
    expect(reportBounds!.x).toBeGreaterThanOrEqual(
      archiveBounds!.x + archiveBounds!.width,
    );
    expect(
      reportBounds!.x - (archiveBounds!.x + archiveBounds!.width),
    ).toBeLessThan(8);
    expect(versionBounds!.x).toBeGreaterThan(
      reportBounds!.x + reportBounds!.width,
    );
    expect(await nativeCalls(page, 'set_admin_mode')).toBe(1);
    await page.keyboard.type('iddqd');
    await expect(reportIssue).toHaveCount(0);
    expect(await nativeCalls(page, 'set_admin_mode')).toBe(2);
  });
});
