# Tests

Deterministic unit and browser coverage for Tau's visible behavior and Pi protocol integration.

## Structure

- `e2e/` - Playwright tests against Vite fixtures or full-app Pi scenarios.
- `e2e/fixtures.ts` - Shared `test`/`expect`, page-error capture, console-error capture, and scenario verification.
- `support/pi-scenario/` - Typed request matchers, scripted outputs, gates, timelines, and scenario catalogue.

## Commands

- `bun run test` - Run the product's Vitest tests.
- `bun run test:e2e` - Run Chromium functional tests only, with two workers. This remains Chromium-only when `CI=1`.
- `bun run test:e2e:webkit` - Run a bounded WebKit startup preflight, then WebKit functional tests only.
- `bun run test:e2e:ci --retries=1` - Preflight WebKit, then run both functional browser projects; this is the CI command.
- `bun run test:e2e:performance` - Run only the two Chromium performance guards: isolated from functional projects, one worker, tracing off, and zero retries.
- `bun x playwright test tests/e2e/<file>.e2e.ts` - Run one e2e file with the Chromium functional default. Select another available project explicitly when needed.
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
- Playwright builds a separate fixture-enabled bundle in `node_modules/.cache/tau-e2e`; normal builds still exclude fixtures. Interactive reproduction continues to use Vite dev. Every browser invocation owns a fresh preview server: an occupied configured port fails rather than reusing its contents. Use `TAU_PLAYWRIGHT_PORT` with an unused port when necessary, and never overlap browser runs in one worktree because that bundle cache is shared.
- Use `?fixture=long-transcript&showcase=true` for rendering checks and `?fixture=long-transcript&preview=true` for file-preview checks; reserve the default 5,000-message fixture for scrolling and virtualization. Install mocks before the test's single navigation, not after a shared navigation that will be discarded.
- Keep test state in the page/context so functional cases can run independently. Functional CI records traces on the first retry; local functional runs retain failure traces. The explicit performance command is already untraced with zero retries. The standard agent macOS VM has a known WebKit startup limitation, so use Chromium for routine local validation and leave required WebKit execution to CI. See `docs/ci-testing.md` for the gates and measurement procedure.
