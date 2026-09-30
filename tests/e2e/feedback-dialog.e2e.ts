import { expect, test } from './fixtures';

async function modShortcut(
  page: import('@playwright/test').Page,
): Promise<string> {
  return (await page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform)))
    ? 'Meta+Enter'
    : 'Control+Enter';
}

test('retries initial feedback dialog focus with Mod+Enter', async ({
  page,
}) => {
  await page.addInitScript(() => {
    let internals: typeof window.__TAURI_INTERNALS__;
    let rejectFirstClaim = true;

    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      configurable: true,
      get: () => internals,
      set: (next) => {
        internals = next;
        if (!internals) return;

        let invoke = internals.invoke;
        Object.defineProperty(internals, 'invoke', {
          configurable: true,
          get: () => invoke,
          set: (handler) => {
            invoke = async (command, ...args): Promise<unknown> => {
              sessionStorage.setItem(
                'feedback-commands',
                `${sessionStorage.getItem('feedback-commands') ?? ''},${command}`,
              );
              if (command === 'claim_pi_frontend' && rejectFirstClaim) {
                rejectFirstClaim = false;
                throw new Error('Simulated ownership failure.');
              }
              return handler(command, ...args);
            };
          },
        });
      },
    });
  });

  await page.goto('/');
  await expect
    .poll(() =>
      page.evaluate(() => sessionStorage.getItem('feedback-commands')),
    )
    .toContain('claim_pi_frontend');

  const dialog = page.getByRole('dialog', { name: 'Pi Unavailable' });
  await expect(dialog).toBeFocused();
  const label = (await modShortcut(page)).startsWith('Meta')
    ? '⌘Enter'
    : 'Ctrl+Enter';
  await expect(
    dialog.getByRole('button', { name: 'Try Again' }),
  ).toHaveAttribute('title', `Try Again (${label})`);
  await page.keyboard.press(await modShortcut(page));

  await expect(dialog).toHaveCount(0);
});
