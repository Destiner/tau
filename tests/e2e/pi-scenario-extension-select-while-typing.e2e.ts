import { expect, test } from './fixtures';
import { releaseGate, waitForGate } from './pi-scenario-gates';

test.use({ pausedClock: true });

test('continued typing cannot answer a Pi select prompt that interrupts a draft', async ({
  page,
}) => {
  await page.goto(
    '/?test-scenario=saved-session-extension-select-while-typing',
  );
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEditable();
  await waitForGate(page, 'before-extension-select');

  const draft = 'Can you include';
  await composer.focus();
  await expect(composer).toBeFocused();
  await page.keyboard.type(draft);
  await expect(composer).toHaveValue(draft);

  await releaseGate(page, 'before-extension-select');
  const prompt = page.getByLabel('Transcript').getByRole('dialog', {
    name: 'Which release label should we use?',
  });
  const first = prompt.getByRole('option', { name: 'patch' });
  const second = prompt.getByRole('option', { name: 'minor' });
  await expect(first).toBeFocused();

  await page.keyboard.press('Space');
  await page.keyboard.type('2.1, too?');

  await expect(prompt).toBeVisible();
  await expect(prompt.getByRole('status')).toHaveCount(0);
  await expect(first).toBeFocused();
  await expect(first).toHaveAttribute('aria-selected', 'true');

  await page.keyboard.press('ArrowDown');
  await expect(second).toBeFocused();
  await page.keyboard.press('Enter');

  await expect(prompt).toHaveCount(0);
  await expect(composer).toHaveValue(draft);
  await expect(composer).toBeFocused();

  const verification = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.verify(),
  );
  expect(verification).toMatchObject({ ok: true });
  expect(
    verification?.timeline.filter(
      (entry) =>
        entry.kind === 'request' &&
        entry.request.type === 'extension_ui_response',
    ),
  ).toMatchObject([{ runtime: 'main', generation: 2 }]);
});
