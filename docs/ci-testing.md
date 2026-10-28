# CI and test execution

## Pull-request gates

`.github/workflows/ci.yml` reports `check` as the aggregate gate for three parallel macOS lanes:

- `frontend`: frozen Bun install, frontend lint, typecheck/build, Chromium install, and Vitest. The rendered transcript-search projection/range tests launch headless Chromium; local `bun run test` also requires `bun x playwright install chromium` once.
- `browser`: frozen Bun install, Chromium/WebKit install, isolated e2e bundle, functional tests in both browsers.
- `native`: frozen Bun install, stable Rust, formatting, Vite bundle, Clippy (all targets, warnings denied), and Cargo tests. Tauri's `generate_context!` needs real `dist` assets even during checks/tests; Bun also runs the Pi stdio adapter in native tests.

`check` runs even if a lane fails or is skipped and succeeds only when all three succeed. PR concurrency cancels older runs for the same PR, not another PR. Installs remain frozen; Bun caches downloads by OS, architecture, and lockfile, not `node_modules`. Rust caches compiler/platform/dependency inputs; Playwright installs the locked browsers. The topology shortens the critical path to the longest lane plus the aggregate job, subject to runner and cache availability; it is not a hosted-CI speed guarantee.

The PR workflow runs `bun run test:e2e:ci --retries=1`: its bounded WebKit preflight must succeed, then both functional browser projects run. Two Chromium performance guards remain outside the PR functional gate. Run them only through `bun run test:e2e:performance`; that isolated command uses one worker, no tracing, and zero retries. The real-Pi contract canary is opt-in (see [reproduction](reproduction.md#real-pi-compatibility-canary)).

## Browser execution

Playwright uses `vite.e2e.config.ts` to build and preview a bundle at `node_modules/.cache/tau-e2e`. It enables only the existing development fixture routes; production `dist` and the normal Vite config remain separate. Interactive `bun run dev` and `bun run repro` are unchanged. Every automated browser invocation builds and owns a fresh preview server, including local runs. An occupied configured port fails rather than silently exercising an unrelated server; set `TAU_PLAYWRIGHT_PORT` to an unused port as the explicit escape hatch. The e2e bundle cache is shared within a worktree, so serialize browser runs there. Build time is included in timed e2e invocations.

`bun run test:e2e` selects Chromium functional coverage only, even when `CI=1`; raw Playwright discovery and focused raw invocations have the same default. `bun run test:e2e:webkit` runs a bounded startup preflight before WebKit functional coverage, while `bun run test:e2e:ci` preflights then selects Chromium and WebKit functional coverage. Discovery with `--list` does not launch WebKit. The standard agent macOS VM is known to hang during WebKit startup, so routine local validation must use Chromium; use the explicit WebKit command only on a functioning desktop or for bounded diagnosis, and leave the required WebKit run to CI.

Functional tests use two workers with `fullyParallel: true`, including across cases within a file. Each gets its own page/context; the shared fixture checks scenario completion and unexpected browser errors. Performance tests remain single-worker. In CI functional traces are recorded on the first retry, not every passing attempt; CI still has one retry, but no trace of the initial failure. Local functional runs retain failure traces. For a CI-only failure use `--trace=retain-on-failure`; the dedicated performance command already disables tracing and retries.

The transcript suites separate large virtualization/scrolling cases (`transcript.e2e.ts`, 5,000 messages), focused preview/copy cases (`transcript-preview.e2e.ts`, one showcase assistant message), and rendering cases (`transcript-rendering.e2e.ts`, 13 showcase messages). `markdown-preview.e2e.ts` uses the one-message preview fixture. `?fixture=long-transcript&showcase=true` reproduces the original last 13 messages without allocating the preceding 4,987; `preview=true` uses the original assistant showcase entry. The remote variant retains its path entry. Keep interaction, reachability, and stability geometry, plus focus, timeout, scroll, process-failure, Pi bootstrap, session-replacement, and draft-preservation assertions in browser tests: lower-layer tests do not replace them. Do not pin cosmetic styling or arbitrary pixel values. Tests of async rendering must wait for the element they measure or click; do not relax bounds or hide races with arbitrary retries.

Tests cover product behavior, not telemetry, CI configuration, release/development tooling, or the test harness itself. Shared fixtures still reject unexpected browser errors and incomplete scenarios.

## Reproducible timing

```sh
bun install --frozen-lockfile
bun x playwright install chromium
bun run test:e2e --list
for run in 1 2 3; do
  /usr/bin/time -p bun run test:e2e --retries=0 --reporter=line || exit $?
done
```

Use three serial Chromium-only functional runs, each with its fresh server, on the same otherwise idle host and with equivalent trace, cache, retry, power, and port conditions on both revisions. Capture per-case JSON durations as well as whole-command wall time. Compare against an equivalent three-run baseline; if only one historical baseline exists, label it honestly as a single-run reference rather than presenting it as a median. On macOS check `pmset -g batt` and `pmset -g custom` (AC does not imply Low Power Mode is off). If port 1420 is occupied, set and record `TAU_PLAYWRIGHT_PORT`. Do not compare retrying with retry-free runs, count installs for only one revision, overlap same-worktree browser commands, or treat a failing short run as an improvement. Cache warmth, runner hardware, queue time, and contention matter. An unchanged performance guard failed at 83–98 ms p95 under AC Low Power Mode but passed five times at 50 ms after disabling it, without altering its threshold or dataset.

For provenance only: an alternating six-run local comparison on 2026-09-27 (Apple M2, AC, Low Power Mode off, zero retries, two workers) measured baseline `b73955a52e9c90b20d536bc5df15a7ef6cfc496b` at 193.43 s median (320 executions) and optimized `5729b7b` at 111.89 s (314 executions), a 42.2% decrease. Both included server startup and optimized bundling. This is not a hosted-CI prediction or a current suite-count guarantee.
