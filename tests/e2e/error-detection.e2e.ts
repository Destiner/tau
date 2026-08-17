/*
 * Proves `./fixtures`'s page/console-error detection actually fails a test,
 * rather than only being exercised by tests that happen to stay clean.
 * Each test below deliberately triggers the failure condition and is marked
 * `test.fail()` (Playwright's "expected to fail" marker): the suite is green
 * only if these two tests themselves fail for the right reason, and red if
 * either one is ever silently swallowed instead.
 */
import { test } from './fixtures';

test('fails when the page throws an uncaught error', async ({ page }) => {
  test.fail(
    true,
    'demonstrates that ./fixtures fails a test on an unexpected page error',
  );
  await page.goto('/?fixture=long-transcript');
  await page.evaluate(() => {
    queueMicrotask(() => {
      throw new Error('tau-e2e-fixture-demo-page-error');
    });
  });
  await page.waitForTimeout(200);
});

test('fails when the page calls console.error', async ({ page }) => {
  test.fail(
    true,
    'demonstrates that ./fixtures fails a test on an unexpected console.error call',
  );
  await page.goto('/?fixture=long-transcript');
  await page.evaluate(() => {
    console.error('tau-e2e-fixture-demo-console-error');
  });
});
