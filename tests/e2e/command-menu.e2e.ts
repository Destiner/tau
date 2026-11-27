import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=command-list-scrolling';

test('slash-command keyboard navigation reveals its selection after manual scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto(scenarioUrl);
  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  const menu = page.locator('.command-menu');
  await composer.evaluate((field: HTMLTextAreaElement) => {
    field.focus();
    field.dispatchEvent(
      new CompositionEvent('compositionstart', { bubbles: true }),
    );
    field.value = '/';
    field.dispatchEvent(
      new InputEvent('input', { bubbles: true, data: '/', isComposing: true }),
    );
    field.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Enter',
        bubbles: true,
        cancelable: true,
        isComposing: true,
      }),
    );
  });
  await expect(composer).toHaveValue('/');
  await expect(menu).toHaveCount(0);
  await composer.dispatchEvent('compositionend', { bubbles: true, data: '/' });
  await expect(menu).toBeVisible();
  await menu.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });

  await composer.press('ArrowUp');
  const selected = menu.locator('.command-option:nth-child(1)');
  await expect(selected).toHaveAttribute('aria-selected', 'true');
  await expect(composer).toHaveAttribute(
    'aria-activedescendant',
    'command-option-0',
  );
  await expect(selected).toBeInViewport();
  await expect(composer).toBeFocused();
});
