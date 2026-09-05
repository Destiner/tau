import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-bootstrap';

test('boots the real app into a deterministic saved session', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  await expect(page.getByRole('heading', { name: 'Main' })).toBeVisible();
  await expect(page.getByText('Tau fixture', { exact: true })).toBeVisible();
  await expect(page.getByText('Main', { exact: true })).toHaveCount(2);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  // An opened session is ready to be typed into without a click first.
  await expect(composer).toBeFocused();
  // The controls are drawn by the app, so what they carry is the label the
  // reader sees rather than the value the model is keyed by.
  await expect(page.getByRole('combobox', { name: 'Model' })).toHaveText(
    'Alpha',
  );
  await expect(
    page.getByRole('combobox', { name: 'Thinking Effort' }),
  ).toHaveText('High');
  await expect(page.getByLabel('Transcript')).toHaveCount(0);
  await expect(page.getByText('Loading', { exact: true })).toHaveCount(0);

  const timeline = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.timeline(),
  );
  expect(timeline).toHaveLength(12);
  expect(timeline?.at(-1)).toMatchObject({
    kind: 'output',
    output: 'response get_messages -> $bootstrap-messages',
  });
});
