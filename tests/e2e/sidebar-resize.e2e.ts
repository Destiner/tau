import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=session-order';

test('fades the resize highlight after a pointer drag', async ({ page }) => {
  await page.goto(fixtureUrl);

  const handle = page.getByRole('separator', { name: 'Resize Sidebar' });
  await expect
    .poll(() =>
      handle.evaluate((element) => {
        const style = getComputedStyle(element, '::after');
        return {
          width: style.width,
          transitionDuration: style.transitionDuration,
        };
      }),
    )
    .toEqual({ width: '1px', transitionDuration: '0.12s' });

  const initialBox = await handle.boundingBox();
  if (!initialBox) throw new Error('Expected the sidebar resize handle.');

  const startX = initialBox.x + initialBox.width / 2;
  const y = initialBox.y + 120;
  await page.mouse.move(startX, y);
  await expect
    .poll(() =>
      handle.evaluate(
        (element) => getComputedStyle(element, '::after').opacity,
      ),
    )
    .toBe('0.4');

  await page.mouse.down();
  await page.mouse.move(startX + 24, y, { steps: 4 });
  await page.mouse.up();

  await expect
    .poll(() =>
      handle.evaluate(
        (element) => getComputedStyle(element, '::after').opacity,
      ),
    )
    .toBe('0');

  await page.mouse.move(startX + 80, y);
  const movedBox = await handle.boundingBox();
  if (!movedBox) throw new Error('Expected the moved sidebar resize handle.');
  await page.mouse.move(movedBox.x + movedBox.width / 2, y);

  await expect
    .poll(() =>
      handle.evaluate(
        (element) => getComputedStyle(element, '::after').opacity,
      ),
    )
    .toBe('0.4');
});
