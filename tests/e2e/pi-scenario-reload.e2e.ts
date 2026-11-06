import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

const progress = 'Reloading Pi resources…';

test('development sandbox models repeated reload without echoing it into the conversation', async ({
  page,
}) => {
  await page.goto('/');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const transcript = page.getByLabel('Transcript');
  await expect(
    page.getByRole('heading', { name: 'Workspace overview', exact: true }),
  ).toBeVisible();
  await expect(transcript.getByRole('article')).toHaveCount(2);
  const conversation = await transcript.innerText();
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await composer.fill('/rel');
    await page
      .getByRole('listbox', { name: 'Commands' })
      .getByRole('option')
      .click();
    await expect(composer).toHaveValue('');
    await expect(
      page.getByRole('combobox', { name: 'Model', exact: true }),
    ).toBeEnabled();
    await expect(transcript).toHaveText(conversation);
    await expect(transcript.getByRole('article')).toHaveCount(2);
  }
  await expect(
    page.getByRole('heading', { name: 'Workspace overview', exact: true }),
  ).toBeVisible();
});

test('reload menu supports keyboard and mouse, refreshes resources, and isolates editable drafts while pending', async ({
  page,
}) => {
  await page.goto('/?test-scenario=reload-resources');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const send = page.getByRole('button', { name: 'Send Message', exact: true });
  const transcript = page.getByLabel('Transcript');
  await expect(transcript).toContainText(
    'The existing conversation stays intact.',
  );

  await composer.fill('/');
  const commands = page.getByRole('listbox', { name: 'Commands' });
  await expect(
    commands.getByRole('option').filter({ hasText: '/reload' }),
  ).toHaveCount(1);
  await expect(commands).not.toContainText('tau-internal');
  await expect(commands).toContainText('/old-resource');
  await composer.fill('/rel');
  await expect(commands.getByRole('option')).toHaveCount(1);
  await composer.press('ArrowDown');
  await composer.press('Enter');
  await waitForGate(page, 'first-pending');
  await expect(page.getByText(progress, { exact: true })).toBeVisible();
  await expect(composer).toBeEnabled();
  await expect(send).toBeDisabled();
  await composer.fill('/reload');
  await composer.press('Enter');
  await composer.press('Enter');
  await composer.fill('Draft edited during reload');
  await expect(composer).toBeFocused();
  await expect(
    page.getByRole('combobox', { name: 'Model', exact: true }),
  ).toBeDisabled();
  await expect(transcript).not.toContainText('/reload');
  await expect(transcript).not.toContainText('tau-internal');

  await releaseGate(page, 'first-pending');
  await waitForGate(page, 'first-refreshing');
  await expect(page.getByText(progress, { exact: true })).toBeVisible();
  await expect(send).toBeDisabled();
  await releaseGate(page, 'first-refreshing');
  await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
  await expect(composer).toHaveValue('Draft edited during reload');
  await expect(composer).toBeFocused();
  await expect(
    page.getByRole('heading', { name: 'Main', exact: true }),
  ).toBeVisible();

  await page.getByRole('combobox', { name: 'Model', exact: true }).click();
  await expect(
    page.getByRole('option', { name: 'Reloaded Beta', exact: true }),
  ).toBeVisible();
  await page.getByRole('searchbox', { name: 'Search Models' }).press('Escape');
  await composer.fill('/');
  await expect(commands).toContainText('/new-resource');
  await expect(commands).not.toContainText('/old-resource');
  await expect(commands).not.toContainText('tau-internal');
  await composer.press('Escape');

  await page.getByRole('button', { name: /^Backup / }).click();
  await expect(
    page.getByRole('heading', { name: 'Backup', exact: true }),
  ).toBeVisible();
  await composer.fill('Backup draft');
  await page.getByRole('button', { name: /^(?:Draft )?Main / }).click();
  await expect(composer).toHaveValue('/');
  await composer.fill('/rel');
  await commands.getByRole('option').click();
  await waitForGate(page, 'second-pending');
  await composer.fill('Main draft during second reload');
  await page.getByRole('button', { name: /^(?:Draft )?Backup / }).click();
  await expect(composer).toHaveValue('Backup draft');
  await composer.fill('Backup remains editable');
  await expect(send).toBeEnabled();
  await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
  await releaseGate(page, 'second-pending');
  await waitForGate(page, 'second-refreshing');
  await releaseGate(page, 'second-refreshing');
  await expect(composer).toHaveValue('Backup remains editable');
  await expect(composer).toBeFocused();
  await expect(
    page.getByRole('heading', { name: 'Backup', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: /^(?:Draft )?Main / }).click();
  await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
  await expect(composer).toHaveValue('Main draft during second reload');
  await expect(transcript).toContainText(
    'The existing conversation stays intact.',
  );
  await expect(transcript).not.toContainText('/reload');
});

for (const editDuringReload of [false, true]) {
  test(`rejected reload preserves ${editDuringReload ? 'newly edited' : 'submitted'} draft without leaking raw failure`, async ({
    page,
  }) => {
    await page.goto('/?test-scenario=reload-rejected');
    const composer = page.getByRole('textbox', { name: 'Message Pi' });
    await composer.fill('  /reload  ');
    // Dismiss autocomplete to exercise exact typed submission, not menu selection.
    await composer.press('Escape');
    await composer.press('Enter');
    await waitForGate(page, 'reload-pending');
    if (editDuringReload) await composer.fill('Keep this newer draft');
    await releaseGate(page, 'reload-pending');
    await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
    await expect(
      page.getByText(
        'Pi rejected the resource reload. Your command was restored.',
        { exact: true },
      ),
    ).toBeVisible();
    await page.getByRole('button', { name: 'Close', exact: true }).click();
    await expect(composer).toHaveValue(
      editDuringReload ? '  /reload  \n\nKeep this newer draft' : '  /reload  ',
    );
    await expect(page.getByText('PRIVATE_RELOAD_FAILURE_CANARY')).toHaveCount(
      0,
    );
    await expect(page.getByLabel('Transcript')).not.toContainText('/reload');
  });
}

test('reload is not sent or queued during streaming or compaction', async ({
  page,
}) => {
  await page.goto('/?test-scenario=reload-busy');
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await waitForGate(page, 'before-streaming');
  await composer.fill('/reload');
  await composer.press('Escape');
  await releaseGate(page, 'before-streaming');
  await waitForGate(page, 'streaming');
  await composer.press('Enter');
  await expect(composer).toHaveValue('/reload');
  await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
  await releaseGate(page, 'streaming');
  await waitForGate(page, 'compacting');
  await composer.press('Enter');
  await expect(composer).toHaveValue('/reload');
  await expect(page.getByText(progress, { exact: true })).toHaveCount(0);
  await releaseGate(page, 'compacting');
  await expect(
    page.getByRole('button', { name: 'Send Message', exact: true }),
  ).toBeEnabled();
  await expect(composer).toHaveValue('/reload');
  await expect(page.getByLabel('Transcript')).not.toContainText('/reload');
});
