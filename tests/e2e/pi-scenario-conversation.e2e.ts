import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-conversation';
const prompt = 'Explain the fixture';
const firstDelta = 'Deterministic';
const completeReply = 'Deterministic reply.';

interface VisibleConversationState {
  transcript: string;
  working: boolean;
}

test('submits and settles a deterministic streamed conversation', async ({
  page,
}) => {
  await page.clock.install();
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();

  await page.evaluate(() => {
    const states: VisibleConversationState[] = [];
    const record = (): void => {
      const transcript =
        document.querySelector('[aria-label="Tau transcript"]')?.textContent ??
        '';
      const working = Boolean(
        document.querySelector(
          '[aria-label="Pi is working"], [aria-label="Working"], [aria-label="Stop Pi"]',
        ),
      );
      const next = { transcript, working };
      const previous = states.at(-1);
      if (
        previous?.transcript !== next.transcript ||
        previous.working !== next.working
      ) {
        states.push(next);
      }
    };
    new MutationObserver(record).observe(document.body, {
      attributes: true,
      childList: true,
      characterData: true,
      subtree: true,
    });
    (
      window as Window & {
        __TAU_VISIBLE_CONVERSATION_STATES__?: VisibleConversationState[];
      }
    ).__TAU_VISIBLE_CONVERSATION_STATES__ = states;
    record();
  });

  await composer.fill(prompt);
  const pauseTarget = await page.evaluate(() => Date.now() + 1_000);
  await page.clock.pauseAt(pauseTarget);
  await page.getByRole('button', { name: 'Send message' }).click();

  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
  await expect(page.getByText(completeReply, { exact: true })).toBeVisible();
  await expect(composer).toHaveValue('');
  await expect(composer).toBeEnabled();
  await expect(
    page.getByRole('button', { name: 'Send message' }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await expect(page.getByRole('status', { name: 'Pi is working' })).toHaveCount(
    0,
  );
  await expect(page.getByRole('img', { name: 'Working' })).toHaveCount(0);
  await expect(
    page.getByText('STALE_GENERATION_SENTINEL', { exact: true }),
  ).toHaveCount(0);

  const visibleStates = await page.evaluate(
    () =>
      (
        window as Window & {
          __TAU_VISIBLE_CONVERSATION_STATES__?: VisibleConversationState[];
        }
      ).__TAU_VISIBLE_CONVERSATION_STATES__ ?? [],
  );
  expect(
    visibleStates.some(
      (state) =>
        state.working &&
        state.transcript.includes(prompt) &&
        !state.transcript.includes(firstDelta),
    ),
    'the optimistic user message should render while Pi is visibly working',
  ).toBe(true);
  expect(
    visibleStates.some(
      (state) =>
        state.transcript.includes(prompt) &&
        state.transcript.includes(firstDelta) &&
        !state.transcript.includes('reply.'),
    ),
    'the first assistant delta should be visible before the second delta',
  ).toBe(true);
  expect(
    visibleStates.some(
      (state) =>
        state.transcript.includes(prompt) &&
        state.transcript.includes(completeReply),
    ),
    'the assistant row should preserve and concatenate both deltas',
  ).toBe(true);

  const timeline = await page.evaluate(() =>
    window.__TAU_PI_SCENARIO__?.timeline(),
  );
  expect(timeline).toHaveLength(26);
  expect(timeline).toContainEqual(
    expect.objectContaining({
      kind: 'request',
      request: { type: 'prompt', message: prompt },
    }),
  );
  expect(timeline?.at(-1)).toMatchObject({
    kind: 'output',
    output: 'response get_messages -> $settled-messages',
  });
});
