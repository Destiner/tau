# Tests

Deterministic unit and browser coverage for Tau's visible behavior and Pi protocol integration.

## Structure

- `e2e/` - Playwright tests against Vite fixtures or full-app Pi scenarios.
- `e2e/fixtures.ts` - Shared `test`/`expect`, page-error capture, console-error capture, and scenario verification.
- `support/pi-scenario/` - Typed request matchers, scripted outputs, gates, timelines, and scenario catalogue.

## Commands

- `bun run test` - Run all Vitest tests, including the scenario engine.
- `bun x vitest run tests/support/pi-scenario/index.test.ts` - Run scenario-engine tests.
- `bun run test:e2e` - Run Chromium and WebKit functional tests, then the dependent isolated performance phase.
- `bun run test:e2e:performance` - Run only the Chromium performance project without its functional dependencies.
- `bun x playwright test tests/e2e/<file>.e2e.ts` - Run one e2e file. `--project=chromium` selects functional Chromium coverage only.
- `bun x playwright test --project=chromium-performance` - Run the performance project after both functional dependencies; add `--no-deps` only for an intentionally isolated performance run.
- `bun run repro -- <scenario>` - Inspect a scenario interactively.

## Patterns

- Import Playwright `test` and `expect` from `e2e/fixtures.ts`; import only types directly from `@playwright/test`.
- Full-app scenario tests select `?test-scenario=<name>`. The shared fixture automatically rejects incomplete scenarios and unexpected browser errors.
- Add browser scenarios once in `support/pi-scenario/catalogue.ts`; the interactive runner and browser adapter share that catalogue.
- Match only meaningful request fields and capture generated request/runtime IDs for later responses.
- Use required gates to expose race windows, and assert visible product behavior rather than controller internals.
- Keep scenarios deterministic and private-data-free. Use the browser fake for product orchestration and the stdio fake for native process/JSONL behavior.
- `bun run test:pi-contract` is an opt-in read-only compatibility canary, not a replacement for fake-based regressions.
- A quality-rubric bug fix must fail before the fix and pass afterward at the narrowest appropriate layer.
