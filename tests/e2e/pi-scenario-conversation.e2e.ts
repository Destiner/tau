import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-conversation';
const prompt = 'Explain the fixture';
const firstDelta = 'Deterministic';
const completeReply = 'Deterministic reply.';

interface VisibleConversationState {
  transcript: string;
  working: boolean;
}

test('keeps the composer editable but blocks repeat sends during delivery', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  const send = page.getByRole('button', { name: 'Send Message' });
  await expect(composer).toBeEnabled();

  await page.evaluate(() => {
    const testWindow = window as typeof window & {
      __TAU_PROMPT_DELIVERY__?: {
        attempts: number;
        release: () => void;
      };
    };
    const internals = window.__TAURI_INTERNALS__;
    if (!internals) throw new Error('Expected mocked Tauri internals.');
    const invoke = internals.invoke;
    let release: (() => void) | undefined;
    const delivery = {
      attempts: 0,
      release: (): void => release?.(),
    };
    testWindow.__TAU_PROMPT_DELIVERY__ = delivery;
    internals.invoke = async (command, args): Promise<unknown> => {
      const request = (args as Record<string, unknown> | undefined)?.request as
        { type?: string } | undefined;
      if (command === 'send_pi' && request?.type === 'prompt') {
        delivery.attempts += 1;
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      return await invoke(command, args);
    };
  });
  await composer.fill(prompt);
  await send.click();
  await page.waitForFunction(
    () =>
      (
        window as Window & {
          __TAU_PROMPT_DELIVERY__?: { attempts: number };
        }
      ).__TAU_PROMPT_DELIVERY__?.attempts === 1,
  );

  await expect(composer).toBeEnabled();
  await expect(composer).toBeFocused();
  await expect(send).toBeDisabled();

  const nextDraft = 'Ask a follow-up';
  await composer.fill(nextDraft);
  await composer.press('Enter');
  await expect(send).toBeDisabled();
  expect(
    await page.evaluate(
      () =>
        (
          window as Window & {
            __TAU_PROMPT_DELIVERY__?: { attempts: number };
          }
        ).__TAU_PROMPT_DELIVERY__?.attempts,
    ),
  ).toBe(1);

  await page.evaluate(() =>
    (
      window as Window & {
        __TAU_PROMPT_DELIVERY__?: { release: () => void };
      }
    ).__TAU_PROMPT_DELIVERY__?.release(),
  );
  await expect(page.getByText(completeReply, { exact: true })).toBeVisible();
  await expect(composer).toHaveValue(nextDraft);
  await expect(send).toBeEnabled();
});

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
        document.querySelector('[aria-label="Transcript"]')?.textContent ?? '';
      const working = Boolean(
        document.querySelector(
          '[aria-label="Working"], [aria-label="Stopping"], [aria-label="Stop Pi"]',
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
  await page.getByRole('button', { name: 'Send Message' }).click();

  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
  await expect(page.getByText(completeReply, { exact: true })).toBeVisible();
  await expect(composer).toHaveValue('');
  await expect(composer).toBeEnabled();
  await expect(
    page.getByRole('button', { name: 'Send Message' }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Stop Pi' })).toHaveCount(0);
  await expect(page.getByRole('status', { name: 'Working' })).toHaveCount(0);
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
