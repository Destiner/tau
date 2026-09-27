# Tests

Deterministic unit and browser coverage for Tau's visible behavior and Pi protocol integration.

## Structure

- `e2e/` - Playwright tests against Vite fixtures or full-app Pi scenarios.
- `e2e/fixtures.ts` - Shared `test`/`expect`, page-error capture, console-error capture, and scenario verification.
- `support/pi-scenario/` - Typed request matchers, scripted outputs, gates, timelines, and scenario catalogue.

## Commands

- `bun run test` - Run the product's Vitest tests.
- `bun run test:e2e` - Run Chromium and WebKit functional tests with two workers (including independent tests within each file), then the dependent single-worker performance phase.
- `CI=1 bun run test:e2e:performance --retries=0` - Run only the Chromium performance project without its functional dependencies or trace-recording overhead.
- `bun x playwright test tests/e2e/<file>.e2e.ts` - Run one e2e file. `--project=chromium` selects functional Chromium coverage only.
- `bun x playwright test --project=chromium-performance` - Run the performance project after both functional dependencies; add `--no-deps` only for an intentionally isolated performance run.
- `bun run repro -- <scenario>` - Inspect a scenario interactively.

## Patterns

- Test product behavior, not telemetry, CI configuration, release/development tooling, or the test harness itself. Keep helpers needed by product tests.
- Import Playwright `test` and `expect` from `e2e/fixtures.ts`; import only types directly from `@playwright/test`.
- Full-app scenario tests select `?test-scenario=<name>`. The shared fixture automatically rejects incomplete scenarios and unexpected browser errors.
- Add browser scenarios once in `support/pi-scenario/catalogue.ts`; the interactive runner and browser adapter share that catalogue.
- Match only meaningful request fields and capture generated request/runtime IDs for later responses.
- Use required gates to expose race windows, and assert visible product behavior rather than controller internals.
- Keep scenarios deterministic and private-data-free. Use the browser fake for product orchestration and the stdio fake for native process/JSONL behavior.
- `bun run test:pi-contract` is an opt-in read-only compatibility canary, not a replacement for fake-based regressions.
- A quality-rubric bug fix must fail before the fix and pass afterward at the narrowest appropriate layer.
- Playwright builds a separate fixture-enabled bundle in `node_modules/.cache/tau-e2e`; normal builds still exclude fixtures. Interactive reproduction continues to use Vite dev.
- Use `?fixture=long-transcript&showcase=true` for rendering checks and `?fixture=long-transcript&preview=true` for file-preview checks; reserve the default 5,000-message fixture for scrolling and virtualization. Install mocks before the test's single navigation, not after a shared navigation that will be discarded.
- Keep test state in the page/context so functional cases can run independently. CI records traces on the first retry; local runs retain failure traces. Use `CI=1` with `--retries=0` for untraced timing/performance measurements. See `docs/ci-testing.md` for the gates and measurement procedure.
