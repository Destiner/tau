import { expect, test } from './fixtures';

test.use({ pausedClock: true });

const scenarioUrl = '/?test-scenario=saved-session-conversation';
const prompt = 'Explain the fixture';
const firstDelta = 'Deterministic';
const completeReply = 'Deterministic reply.';

interface VisibleConversationState {
  transcript: string;
  working: boolean;
}

test('wraps an unbroken link without widening the composer', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await composer.fill(`https://example.com/${'a'.repeat(10_000)}`);

  const draftIndicator = page.locator(
    '.session-row .ui-status-dot[aria-label="Unsent draft"]',
  );
  await expect(draftIndicator).toHaveCSS('width', '6px');
  await expect(draftIndicator).toHaveCSS('height', '6px');
  await expect(draftIndicator).toHaveCSS(
    'background-color',
    'rgb(110, 117, 127)',
  );
  await expect(draftIndicator).toHaveCSS('opacity', '0.3');
  // Lifted three quarters of a pixel off the title's baseline, and by a
  // transform: margin cannot move a baseline-aligned empty flex item, and a
  // relative offset rounds the sub-pixel away differently per engine.
  await expect(draftIndicator).toHaveCSS(
    'transform',
    'matrix(1, 0, 0, 1, 0, -0.75)',
  );

  await expect
    .poll(() =>
      page.evaluate(() => {
        const input = document.querySelector<HTMLTextAreaElement>(
          'textarea[aria-label="Message Pi"]',
        );
        if (!input) return false;
        return (
          document.body.scrollWidth === window.innerWidth &&
          input.scrollWidth === input.clientWidth &&
          input.scrollHeight > input.clientHeight
        );
      }),
    )
    .toBe(true);

  // Complete the deterministic scenario after checking the unsent draft.
  await composer.fill(prompt);
  await composer.press('Enter');
  await expect(page.getByText(completeReply, { exact: true })).toBeVisible();
});

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
  await expect(composer).toHaveValue('');
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

test('restores an immediately cleared composer when delivery fails', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.locator('textarea[aria-label="Message Pi"]');
  const send = page.locator('button[aria-label="Send Message"]');
  await page.waitForFunction(() => Boolean(window.__TAURI_INTERNALS__));
  await page.evaluate(() => {
    const testWindow = window as typeof window & {
      __TAU_FAILED_PROMPT_DELIVERY__?: {
        attempts: number;
        reject: () => void;
      };
    };
    const internals = window.__TAURI_INTERNALS__;
    if (!internals) throw new Error('Expected mocked Tauri internals.');
    const invoke = internals.invoke;
    let rejectDelivery: (() => void) | undefined;
    const delivery = {
      attempts: 0,
      reject: (): void => rejectDelivery?.(),
    };
    testWindow.__TAU_FAILED_PROMPT_DELIVERY__ = delivery;
    internals.invoke = async (command, args): Promise<unknown> => {
      const request = (args as Record<string, unknown> | undefined)?.request as
        { type?: string } | undefined;
      if (command === 'send_pi' && request?.type === 'prompt') {
        delivery.attempts += 1;
        if (delivery.attempts === 1) {
          await new Promise<void>((resolve) => {
            rejectDelivery = resolve;
          });
          throw new Error('Simulated prompt delivery failure');
        }
      }
      return await invoke(command, args);
    };
  });

  await composer.fill(`  ${prompt}  `);
  await send.click();
  await page.waitForFunction(
    () =>
      (
        window as Window & {
          __TAU_FAILED_PROMPT_DELIVERY__?: { attempts: number };
        }
      ).__TAU_FAILED_PROMPT_DELIVERY__?.attempts === 1,
  );
  await expect(composer).toHaveValue('');
  await expect(send).toBeDisabled();

  await page.evaluate(() =>
    (
      window as Window & {
        __TAU_FAILED_PROMPT_DELIVERY__?: { reject: () => void };
      }
    ).__TAU_FAILED_PROMPT_DELIVERY__?.reject(),
  );
  await expect(composer).toHaveValue(`  ${prompt}  `);
  const feedback = page.getByRole('dialog', { name: 'Message Not Sent' });
  await expect(feedback).toContainText(
    'The message could not be sent. Reopen the session and try again.',
  );
  await expect(page.locator('.status-row')).toHaveCount(0);
  await feedback.getByRole('button', { name: 'Close' }).click();
  await expect(composer).toBeFocused();
  await expect(send).toBeEnabled();
  await expect(page.getByText(prompt, { exact: true })).toHaveCount(0);

  await send.click();
  await expect(page.getByText(completeReply, { exact: true })).toBeVisible();
  await expect(composer).toHaveValue('');
});

test('submits and settles a deterministic streamed conversation', async ({
  page,
}) => {
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
