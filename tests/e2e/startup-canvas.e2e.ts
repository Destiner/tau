import { expect, test } from '@playwright/test';

for (const scheme of ['dark', 'light'] as const) {
  test(`paints the ${scheme} canvas before the frontend loads`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.route('**/*', (route) => {
      const type = route.request().resourceType();
      return type === 'script' || type === 'stylesheet'
        ? route.abort()
        : route.continue();
    });
    await page.goto('/');

    const canvas = page.locator('html');
    const colors = { dark: 'rgb(16, 20, 28)', light: 'rgb(252, 252, 252)' };
    await expect(page.locator('#app')).toBeEmpty();
    await expect(canvas).toHaveCSS('background-color', colors[scheme]);
    await expect(canvas).toHaveCSS('color-scheme', 'light dark');

    const nextScheme = scheme === 'dark' ? 'light' : 'dark';
    await page.emulateMedia({ colorScheme: nextScheme });
    await expect(canvas).toHaveCSS('background-color', colors[nextScheme]);
  });
}
