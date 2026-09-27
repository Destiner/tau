# CI and test execution

## Pull-request gates

`.github/workflows/ci.yml` reports `check` as the aggregate gate for three parallel macOS lanes:

- `frontend`: frozen Bun install, frontend lint, typecheck/build, and Vitest.
- `browser`: frozen Bun install, Chromium/WebKit install, isolated e2e bundle, functional tests in both browsers.
- `native`: frozen Bun install, stable Rust, formatting, Vite bundle, Clippy (all targets, warnings denied), and Cargo tests. Tauri's `generate_context!` needs real `dist` assets even during checks/tests; Bun also runs the Pi stdio adapter in native tests.

`check` runs even if a lane fails or is skipped and succeeds only when all three succeed. PR concurrency cancels older runs for the same PR, not another PR. Installs remain frozen; Bun caches downloads by OS, architecture, and lockfile, not `node_modules`. Rust caches compiler/platform/dependency inputs; Playwright installs the locked browsers. The topology shortens the critical path to the longest lane plus the aggregate job, subject to runner and cache availability; it is not a hosted-CI speed guarantee.

The PR workflow runs functional browser projects only. Two Chromium performance guards run separately with a single worker after functional projects in `bun run test:e2e`; validate in isolation with `CI=1 bun run test:e2e:performance --retries=0`. The real-Pi contract canary is opt-in (see [reproduction scenarios](reproduction-scenarios.md#real-pi-compatibility-canary)).

## Browser execution

Playwright uses `vite.e2e.config.ts` to build and preview a bundle at `node_modules/.cache/tau-e2e`. It enables only the existing development fixture routes; production `dist` and the normal Vite config remain separate. Interactive `bun run dev` and `bun run repro` are unchanged. `CI=1` always starts its own server; local runs may reuse one. Build time is included in timed e2e invocations.

Functional tests use two workers with `fullyParallel: true`, including across cases within a file. Each gets its own page/context; the shared fixture checks scenario completion and unexpected browser errors. Performance tests remain single-worker. In CI traces are recorded on the first retry, not every passing attempt; CI still has one retry, but no trace of the initial failure. Local runs retain failure traces. For a CI-only failure use `--trace=retain-on-failure`. For uncontaminated performance timings use `CI=1` and `--retries=0`.

The transcript suites separate large virtualization/scrolling cases (`transcript.e2e.ts`, 5,000 messages), focused preview/copy cases (`transcript-preview.e2e.ts`, one showcase assistant message), and rendering cases (`transcript-rendering.e2e.ts`, 13 showcase messages). `markdown-preview.e2e.ts` uses the one-message preview fixture. `?fixture=long-transcript&showcase=true` reproduces the original last 13 messages without allocating the preceding 4,987; `preview=true` uses the original assistant showcase entry. Unit tests protect fixture parity. The remote variant retains its path entry. Browser-error guards inject errors in a fresh blank page. Keep geometry, focus, timeout, scroll, process-failure, Pi bootstrap, session-replacement, and draft-preservation assertions in browser tests: lower-layer tests do not replace them. Tests of async rendering must wait for the element they measure or click; do not relax bounds or hide races with arbitrary retries.

`tests/ci-workflow.test.ts` verifies the gate's commands, setup, and all 64 success/failure/cancelled/skipped combinations. `tests/playwright-config.test.ts` checks worker scheduling, performance dependencies, and fixture isolation from production.

## Reproducible timing

```sh
bun install --frozen-lockfile
bun x playwright install chromium webkit
bun x playwright test --project=chromium --project=webkit --list
for run in 1 2 3; do
  /usr/bin/time -p env CI=1 bun x playwright test \
    --project=chromium --project=webkit --retries=0 --reporter=line || exit $?
done
```

Use the same otherwise idle host and power settings on both revisions; on macOS check `pmset -g batt` and `pmset -g custom` (AC does not imply Low Power Mode is off). If port 1420 is occupied, set and record `TAU_PLAYWRIGHT_PORT`. Do not compare retrying with retry-free runs, count installs for only one revision, or treat a failing short run as an improvement. Cache warmth, runner hardware, queue time, and contention matter. An unchanged performance guard failed at 83–98 ms p95 under AC Low Power Mode but passed five times at 50 ms after disabling it, without altering its threshold or dataset.

For provenance only: an alternating six-run local comparison on 2026-09-27 (Apple M2, AC, Low Power Mode off, zero retries, two workers) measured baseline `b73955a52e9c90b20d536bc5df15a7ef6cfc496b` at 193.43 s median (320 executions) and optimized `5729b7b` at 111.89 s (314 executions), a 42.2% decrease. Both included server startup and optimized bundling. This is not a hosted-CI prediction or a current suite-count guarantee.
