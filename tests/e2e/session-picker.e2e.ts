import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-picker';

test.describe('Switch Session fixture', () => {
  test('initially reveals the active session in a later group', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(fixtureUrl);

    const picker = page.getByRole('dialog', { name: 'Switch Session' });
    const results = picker.locator('.command-palette-results');
    const current = picker.getByRole('option', {
      name: 'Duplicate session Current',
    });
    await expect(current).toHaveAttribute('aria-selected', 'true');
    await expect
      .poll(() => results.evaluate((element) => element.scrollTop))
      .toBeGreaterThan(0);
    await expect(current).toBeInViewport();
  });

  test('distinguishes duplicate project names and session IDs', async ({
    page,
  }) => {
    await page.goto(fixtureUrl);

    const picker = page.getByRole('dialog', { name: 'Switch Session' });
    const groups = picker.getByRole('group');
    await expect(groups).toHaveCount(5);
    await expect(
      groups.evaluateAll((elements) =>
        elements.map((group) => group.getAttribute('aria-label')),
      ),
    ).resolves.toEqual([
      'Earlier A',
      'Earlier B',
      'Duplicate project',
      'Duplicate project',
      'Remote project',
    ]);

    await groups
      .nth(3)
      .getByRole('option', { name: 'Duplicate session Current' })
      .click();
    await expect(page.getByTestId('selected-session')).toHaveText(
      '/fixture/duplicate-b/shared-session',
    );
  });

  test('does not navigate when the clicked target is removed', async ({
    page,
  }) => {
    await page.goto(fixtureUrl);
    const picker = page.getByRole('dialog', { name: 'Switch Session' });

    await page.evaluate(() => {
      window.__TAU_SESSION_PICKER_FIXTURE__?.armStaleRemoval(
        '/fixture/duplicate-b',
        'shared-session',
      );
    });
    await picker
      .getByRole('group')
      .nth(3)
      .getByRole('option', { name: 'Duplicate session Current' })
      .click();
    await expect(picker).toBeVisible();
    await expect(page.getByTestId('selected-session')).toHaveText(
      '/fixture/duplicate-b/shared-session',
    );
  });

  test('repairs the highlight after live collapse and clears it when empty', async ({
    page,
  }) => {
    await page.goto(fixtureUrl);
    const picker = page.getByRole('dialog', { name: 'Switch Session' });
    const search = picker.getByRole('combobox', { name: 'Switch Session' });
    await expect(
      picker.getByRole('option', { name: 'Duplicate session Current' }),
    ).toHaveAttribute('aria-selected', 'true');
    await page.evaluate(() =>
      window.__TAU_SESSION_PICKER_FIXTURE__?.collapseProject(
        '/fixture/duplicate-b',
      ),
    );
    await expect(
      picker.getByRole('option', { name: 'Earlier A 0' }),
    ).toHaveAttribute('aria-selected', 'true');
    await search.fill('Remote destination');
    await expect(
      picker.getByRole('option', { name: 'Remote destination just now' }),
    ).toHaveAttribute('aria-selected', 'true');
    await search.fill('missing result');
    await expect(picker.getByRole('option')).toHaveCount(0);
    await expect(search).not.toHaveAttribute('aria-activedescendant');
    await expect(picker.getByText('No matches')).toBeVisible();
    await search.fill('');
    await page.evaluate(() =>
      window.__TAU_SESSION_PICKER_FIXTURE__?.collapseAll(),
    );
    await expect(picker.getByText('No sessions')).toBeVisible();
    await expect(search).not.toHaveAttribute('aria-activedescendant');
    await search.press('Enter');
    await expect(picker).toBeVisible();
  });

  test('selects a remote destination without a live Pi runtime', async ({
    page,
  }) => {
    await page.goto(fixtureUrl);

    const picker = page.getByRole('dialog', { name: 'Switch Session' });
    await picker
      .getByRole('group', { name: 'Remote project' })
      .getByRole('option', { name: 'Remote destination just now' })
      .click();
    await expect(page.getByTestId('selected-session')).toHaveText(
      '/fixture/remote/remote-session',
    );
  });
});
