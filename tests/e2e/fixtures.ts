/*
 * Stage 6: browser tests must fail on an unexpected page error or
 * `console.error` call, not just on the specific assertions each test
 * writes. Every e2e test should import `test`/`expect` from here instead of
 * `@playwright/test` directly, so this check applies uniformly rather than
 * being repeated (and inevitably forgotten) per test file.
 */
import { test as base, expect } from '@playwright/test';

/** WebKit reports the browser's own benign `ResizeObserver loop limit
 * exceeded`/`ResizeObserver loop completed with undelivered notifications`
 * notification as a `pageerror` with no stack — not an application error,
 * and not something any app code can catch or prevent (it is dispatched by
 * the browser's layout engine after a resize-observer callback loop, not
 * thrown from a script frame; Chromium does not surface it as a page error
 * at all). Matching on the message text, not "any error with an empty
 * stack", keeps this from accidentally swallowing a real thrown non-Error
 * value that also happens to lack a stack. */
function isKnownBenignBrowserNotification(error: Error): boolean {
  return !error.stack && /^ResizeObserver loop /.test(error.message);
}

const test = base.extend({
  page: async ({ page }, use) => {
    const pageErrors: Error[] = [];
    const consoleErrors: string[] = [];

    page.on('pageerror', (error) => {
      if (isKnownBenignBrowserNotification(error)) return;
      pageErrors.push(error);
    });
    page.on('console', (message) => {
      if (message.type() === 'error') consoleErrors.push(message.text());
    });

    await use(page);

    const failures: string[] = [];
    const scenario = await page.evaluate(() =>
      window.__TAU_PI_SCENARIO__?.verify(),
    );
    if (scenario && !scenario.ok) {
      failures.push(
        `Pi scenario mismatch:\n${scenario.error ?? 'Unknown scenario failure.'}\nTimeline:\n${scenario.timeline
          .map((entry) => JSON.stringify(entry))
          .join('\n')}`,
      );
    }
    if (pageErrors.length > 0) {
      failures.push(
        `Test produced ${pageErrors.length} unexpected page error(s):\n${pageErrors
          .map((error) => error.stack ?? error.message)
          .join('\n---\n')}`,
      );
    }
    if (consoleErrors.length > 0) {
      failures.push(
        `Test produced ${consoleErrors.length} unexpected console.error call(s):\n${consoleErrors.join('\n---\n')}`,
      );
    }
    if (failures.length > 0) throw new Error(failures.join('\n---\n'));
  },
});

export { expect, test };
