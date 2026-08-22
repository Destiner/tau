import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=extension-dialog';

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto(fixtureUrl);
});

test('opens the transcript at a question larger than the pane', async ({
  page,
}) => {
  const transcript = page.getByLabel('Tau transcript');
  const prompt = page.getByRole('dialog');
  await expect(prompt).toBeVisible();

  const transcriptBox = await transcript.boundingBox();
  const promptBox = await prompt.boundingBox();
  expect(transcriptBox).not.toBeNull();
  expect(promptBox).not.toBeNull();
  if (!transcriptBox || !promptBox) return;

  // The prompt is taller than the pane, and the transcript is scrolled to it:
  // its end is the end of the scroll region, and the question fills the view.
  expect(promptBox.height).toBeGreaterThan(transcriptBox.height);
  const scroll = await transcript.evaluate((element) => ({
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  expect(
    scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight,
  ).toBeLessThanOrEqual(2);
});

test('scrolls the question, its options, and the transcript as one region', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const options = prompt.getByRole('listbox');
  const transcript = page.getByLabel('Tau transcript');

  // Only the transcript scrolls: nothing inside the prompt is a region of
  // its own, so the question, the options, and the history read as one view.
  for (const region of [prompt, options]) {
    const overflow = await region.evaluate((element) => ({
      scrollHeight: element.scrollHeight,
      clientHeight: element.clientHeight,
    }));
    expect(overflow.scrollHeight).toBe(overflow.clientHeight);
  }

  const transcriptOverflow = await transcript.evaluate((element) => ({
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  expect(transcriptOverflow.scrollHeight).toBeGreaterThan(
    transcriptOverflow.clientHeight,
  );

  // The messages that came before the question are still reachable above it.
  await transcript.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByText('History prompt 0', { exact: true }),
  ).toBeVisible();
});

test('renders a table in the question as a table', async ({ page }) => {
  const prompt = page.getByRole('dialog');
  const header = prompt.locator('.extension-prompt-message th').first();
  await expect(header).toBeVisible();

  const style = await header.evaluate((element) => {
    const computed = window.getComputedStyle(element);
    return {
      // Rows are ruled, columns are not: the header's rule is its underline.
      rule: Number.parseFloat(computed.borderBottomWidth),
      columnRule: Number.parseFloat(computed.borderRightWidth),
      padding: Number.parseFloat(computed.paddingLeft),
      background: computed.backgroundColor,
      textAlign: computed.textAlign,
    };
  });
  expect(style.rule).toBeGreaterThan(0);
  expect(style.columnRule).toBe(0);
  expect(style.padding).toBeGreaterThan(0);
  expect(style.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(style.textAlign).toBe('left');
});

test('scrolls a table wider than the question inside itself', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const table = prompt.locator('.extension-prompt-message table');

  const width = await table.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(width.scrollWidth).toBeGreaterThan(width.clientWidth);

  // The table takes the sideways scrolling, so the prompt keeps its own width.
  const promptWidth = await prompt.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(promptWidth.scrollWidth).toBe(promptWidth.clientWidth);
});

test('reaches an option below the fold and reports the choice', async ({
  page,
}) => {
  const prompt = page.getByRole('dialog');
  const option = prompt.getByRole('option', { name: 'label-11' });

  await option.scrollIntoViewIfNeeded();
  await option.click();

  await expect(page.getByTestId('dialog-outcome')).toHaveText(
    '{"submit":"label-11"}',
  );
  await expect(prompt).toBeHidden();
});
