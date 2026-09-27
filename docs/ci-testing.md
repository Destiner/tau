# CI and test execution

## Pull-request gates

CI keeps the `check` result as the aggregate gate. Three independent macOS lanes
run in parallel:

- `frontend`: frozen Bun install, all frontend linters, typechecking/production
  build, and Vitest.
- `browser`: frozen Bun install, Chromium/WebKit installation, an isolated
  browser-test bundle, and all functional tests in both browsers.
- `native`: frozen Bun install, stable Rust, formatting, a Vite bundle, Clippy
  (all targets, warnings denied), and Cargo tests. Tauri's `generate_context!`
  requires real `dist` assets even during Cargo checks/tests. Bundling here
  avoids waiting for the frontend lane; typechecking is not repeated. Bun also
  runs `scripts/pi-stdio-adapter.ts` in the native process tests.

`check` runs even when a dependency fails or is skipped, and accepts only three
successful lane results. Cancellation cannot produce a successful gate. New
runs cancel older runs for the same PR, not runs belonging to another PR. No
repository branch-protection settings or release workflow changes are needed.

Bun's download cache is keyed by OS, architecture, and lockfile; installs remain
frozen and `node_modules` is not shared between jobs. The Rust cache action keys
its cache by compiler/platform/dependency inputs, with an explicit macOS runner
and lockfile key and the `src-tauri` workspace. Browser binaries are installed by
the locked Playwright version rather than introducing another large cache whose
restore cost may rival the download.

Previously, one macOS job performed all of these checks sequentially without
caches. The new critical path is the longest lane plus the aggregate job, rather
than the sum of all lanes, subject to runner availability and setup/cache costs.
This is a topology improvement, not a measured hosted-CI speedup. No GitHub run
was triggered for this change.

The two Chromium performance guards remain a separate single-worker phase after
both functional projects in `bun run test:e2e`. As before, the PR workflow runs
functional projects only. Performance thresholds and datasets are unchanged;
run `CI=1 bun run test:e2e:performance --retries=0` for isolated validation. The real-Pi contract
canary remains opt-in.

## Browser scheduling and coverage

Playwright builds with `vite.e2e.config.ts` and serves Vite preview instead of
loading the development-module graph on each fresh page. The separate
config enables only the existing `import.meta.env.DEV` fixture routes and writes
to `node_modules/.cache/tau-e2e`, never production `dist`. Normal frontend/native
builds use the original config and still exclude fixture/scenario code. Both
engines exercise the same real components and deterministic native/Pi adapters.
The build cost is included in every timed invocation, not hidden as pre-work.
Interactive `bun run dev` / `bun run repro` remains unchanged. Outside CI,
Playwright can still reuse an existing local server; `CI=1` always starts its
own bundled server.

Functional tests can use both workers within a file (`fullyParallel: true`),
not just across files. The worker limit remains two: this removes the large
serial-file tail without increasing resource contention. Each test still gets
its own context/page, and scenario completion and browser-error checks still
run through the shared fixture. Removing tracing exposed readiness assumptions:
geometry checks now wait for the measured image/code/prose to be visible, and
the long-link composer case awaits strict scenario completion after the final
delta. No geometry bounds or scenario expectations were relaxed. Performance
tests retain one worker and their functional-project dependencies.

With `CI=1`, traces are recorded only on the first retry, rather than recorded
for every test and discarded on success. The workflow's existing single retry
is unchanged; retry-free measurements record no traces. Unexpected browser
errors, scenario mismatches, assertions, and failure context still fail/report
normally. The diagnostic trade-off is that CI no longer has a trace of the
first failed attempt. Local runs retain failure traces by default; use
`--trace=retain-on-failure` explicitly when investigating a CI-only failure.
Trace recording itself can perturb the frame-delivery benchmark, so performance
validation uses `CI=1` and zero retries as well.

The original 45-case `transcript.e2e.ts` loaded 5,000 messages before every case;
12 preview/copy cases then navigated again after installing native mocks. It is
now divided by purpose:

- `transcript.e2e.ts`: the same 14 scrolling/virtualization/image-growth cases,
  original 5,000-message dataset and geometry/DOM bounds. Session restoration now
  waits for `scrollend` before capturing the starting anchor. The restoration
  check still uses its original 150 ms settling wait and immediate same-message,
  less-than-2-pixel assertion; its acceptance window was not extended.
- `transcript-preview.e2e.ts`: 12 file-preview/copy cases, one navigation after
  mock installation, using only the original assistant showcase message.
- `transcript-rendering.e2e.ts`: 16 rendering cases after consolidation, using
  the 13-message showcase.
- `markdown-preview.e2e.ts`: all 12 existing cases retained with the one-message
  preview fixture rather than 5,000 unrelated messages.

`?fixture=long-transcript&showcase=true` constructs exactly the original last 13
messages, with unchanged IDs, content and ordering, without constructing the
preceding 4,987 entries. The remote variant still appends its original path
message. Unit tests compare the showcase with the original tail and check
independent entries and the unchanged default size. This does not change the
production app or replace browser rendering assertions with unit assertions.
`?fixture=long-transcript&preview=true` constructs just the original assistant
showcase entry for preview tests (one local message, two with the remote path
entry). Its contents are also checked against the original fixture. The delayed
preview test waits for initial diagrams, highlighting and fonts before testing
its pending-request feedback, rather than racing unrelated initial rendering.

The two browser-error guard tests now inject errors into their fresh blank page
rather than loading the entire transcript. Waiting for the actual error event
replaces the fixed 200 ms sleep. Their expected-failure checks still prove that
the shared fixture rejects page errors and console errors. Negative-observation
and scrolling settling waits elsewhere were retained rather than blindly
removed.

Prompt expiry uses a paused browser clock: the prompt must still exist one
millisecond before its original timeout, then disappear at the deadline and
restore composer focus. The other prompt interactions share that paused clock
so assertion speed cannot accidentally expire them. The first-run loading test
still observes the pending preparation UI before advancing its original
three-second ownership delay. Neither test shortens a product timeout or skips
its transitional-state assertions.

### Removal/consolidation ledger

| Original standalone cases                                                                                                                                                                                   | Retained coverage                                                                                                                                                                                                                                                     | Why consolidate                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `browser-sandbox.e2e.ts`: “starts the browser sandbox with two projects and five sessions”                                                                                                                  | “replies locally and restores the browser sandbox seed on reload” now checks initial project/session counts, selected row, and opening text before sending, then the original reload assertions. `src/dev/browser-sandbox.test.ts` also retains seed isolation tests. | The reload journey already boots this same sandbox; a separate seed-only browser load adds no distinct interaction. |
| `transcript.e2e.ts`: “marks only the calls that are running or failed”; “names the failed result as an error when the row is opened”                                                                        | `transcript-rendering.e2e.ts`: “marks running and failed calls and names the failed result as an error” retains mark presence, sizes, alignment, expanded labels, and error body assertions.                                                                          | One status-row journey can inspect the collapsed and expanded states without reloading.                             |
| `transcript.e2e.ts`: “aligns table columns the way the markdown asked”; “omits a wholly empty table header but keeps partial headers and alignment”; “keeps a table header visible against the user bubble” | `transcript-rendering.e2e.ts`: “aligns table columns and preserves empty and user-bubble headers” retains all alignment, empty/partial header, and user-bubble contrast assertions.                                                                                   | These are read-only checks on the same showcase tables; three independent app loads were redundant.                 |

Four standalone cases per browser were consolidated away. Separately,
`extension-dialog.e2e.ts` now runs the submitting/disabled Escape states as two
isolated cases instead of navigating twice within one case. Both sets of
assertions remain; a new context avoids aborting the old page's pending syntax
highlighter import. The net count is **160 → 157** per browser, or **320 → 314**
functional executions across 44 → 46 files. All unique browser assertions are
retained. Smaller fixtures and eliminated navigation reduce work beyond this
count reduction.

`transcript-disclosure.e2e.ts` also no longer loads and discards a compact
transcript before navigating to the verbose tool fixture. Its compact-transcript
preconditions remain in the two cases that use that fixture. Markdown preview
focus tests wait for the original transcript markup to finish rendering before
capturing the opening control; compaction tests wait for fonts and an in-viewport
button before measuring its position. The remote extension-title path test also
waits for title highlighting before clicking a control that the asynchronous
markup replacement would otherwise remove. These checks retain their original
copy/focus/geometry assertions and the shared browser-error guard.
CSS, focus, layout, security, orchestration, and large-session regressions were
not removed just because lower-layer tests exist. In particular, Pi bootstrap,
process failure, draft preservation, and session replacement scenarios remain
unchanged.

### Configuration regression tests

`tests/ci-workflow.test.ts` parses the workflow, checks required commands and
setup dependencies, and executes the aggregate shell against all 64 combinations
of success/failure/cancelled/skipped. These cases share one Bash process rather
than repeatedly starting a shell. `tests/playwright-config.test.ts` checks the
two-worker functional schedule and the dependent single-worker performance
phase. It also resolves both Vite configs to check that fixture hooks and the
browser-test output stay separate from the production build.

The real-server launcher test now allocates an IPv4 loopback port, then forces
its second server to collide with that owned port. It still verifies distinct
reachable URLs, lifetime, and cleanup, without accidentally contacting another
checkout's IPv4 server via `localhost` after its own IPv6 listener closes.

## Reproduce the measurements

Install dependencies and browsers before timing:

```sh
bun install --frozen-lockfile
bun x playwright install chromium webkit
bun x playwright test --project=chromium --project=webkit --list
```

Run on the same, otherwise idle host, without simultaneous builds/tests. Record
the active power source and disable Low Power Mode for both revisions. On macOS,
`pmset -g batt` reports the source and `pmset -g custom` reports settings for each
source; being plugged in does not mean Low Power Mode is off.
`CI=1` forbids silently reusing an unrelated server; if 1420 is occupied, set
`TAU_PLAYWRIGHT_PORT` to a free port and record it. Each iteration starts a new
server and browser workers/contexts:

```sh
for run in 1 2 3; do
  echo "Functional browser timing run $run"
  /usr/bin/time -p env CI=1 bun x playwright test \
    --project=chromium --project=webkit --retries=0 --reporter=line \
    || exit $?
done
```

Do not compare a retrying run with a retry-free run or include dependency install
time in one sample but not the other. Cache hits, hosted runner hardware, queue
time, and concurrent machine load can change results. Passing without retries
is required for the measured optimized samples; a shorter failing run is not an
improvement.

## Measurement record

### Controlled final comparison

Measured on 2026-09-27. Baseline revision:
`b73955a52e9c90b20d536bc5df15a7ef6cfc496b`. Local host: Apple M2, 8 GiB RAM,
macOS 26.6.2, Bun 1.3.0, Node 26.8.2, Playwright 1.62.1. Both browser versions
came from the unchanged lockfile.

The final comparison alternated baseline 1 / optimized 1 / baseline 2 /
optimized 2 / baseline 3 / optimized 3 on this same host. Every run used two
workers, `CI=1`, the line reporter, zero retries and `TAU_PLAYWRIGHT_PORT=1431`.
AC power and Low Power Mode off were verified before and after every run. The
monitor required 60 seconds without external Playwright/Cargo work before each
run and detected no such overlap during any of the six runs. No other builds
or tests were launched by this task alongside them.

The original source was archived inside the worktree, with every tracked file
verified byte-for-byte against the baseline commit and its own frozen-installed
dependencies. Run executable TypeScript snapshots outside `node_modules`, whose
special loader treatment prevents Playwright config loading. Dependency/browser
installation was excluded for both versions. Dependency and Vite prebundle caches
were left warm; each invocation started a fresh server and browser workers.
Baseline used Vite dev; optimized rebuilt and served its isolated bundle every
time. The build cost is included, not hidden in pre-work.

| Configuration      | Executions/run | Run 1 (s) | Run 2 (s) | Run 3 (s) | Median (s) |
| ------------------ | -------------: | --------: | --------: | --------: | ---------: |
| Unchanged baseline |            320 |    193.43 |    187.40 |    194.51 |     193.43 |
| Optimized          |            314 |    112.28 |    111.89 |    111.36 |     111.89 |

All six runs passed without retries. The measured local median decreased by
**81.54 seconds (42.2%)**. These are local measurements, not hosted macOS-15 CI
predictions. The sample is small, ordinary desktop activity is not controlled,
and hosted hardware, queueing and cache availability can change the result.

### Initial measurements and rejected comparisons

The required unchanged baseline was also captured before implementation. These
original runs and an earlier complete optimized set all passed, but their power
state was not recorded. They are retained here for transparency, not used for
the controlled speedup above:

| Earlier snapshot            | Executions/run | Run 1 (s) | Run 2 (s) | Run 3 (s) | Median (s) |
| --------------------------- | -------------: | --------: | --------: | --------: | ---------: |
| Pre-implementation baseline |            320 |    202.61 |    220.49 |    261.72 |     220.49 |
| Earlier optimized           |            314 |    126.55 |    127.24 |    128.80 |     127.24 |

An intermediate 312-case profile, after fixture/scheduling changes but before
bundling, identified expensive WebKit specs: transcript scrolling 32.86 s,
transcript rendering 24.47 s, Markdown preview 20.88 s, and file preview 18.81 s.
These are sums of per-test durations, not additional wall time: workers overlap.
They motivated shared loading/setup improvements, not dropping a browser.

Exploratory results were not uniformly faster. Before final tracing changes,
one run passed in 180.18 s, then a three-run set took 219.59 / 258.02 / 267.06 s
(median 258.02 s): not an accepted speedup. Other checkouts were observed running
browser tests and occupying port 1420. An archived-baseline attempt failed
WebKit compaction checks at 400.37 s. A later control passed at 204.11 / 188.72 s,
then failed four WebKit remote-dialog/tooltip/sidebar timeouts at 390.79 s.
Neither incomplete control is used above; their unrecorded power state prevents
assigning a cause retrospectively. Baseline assertions were never changed.

Faster loading also exposed setup races. One incomplete optimized set passed at
116.41 s then failed at 131.96 s during hover. The first power-controlled attempt
passed baselines at 193.49 / 187.73 s and optimized at 109.52 s, then failed at
114.51 s when title highlighting replaced a path control during its click.
Explicit initial-render readiness repaired these races without retrying actions
or changing the copy/focus/geometry assertions. Each repair passed 40 focused
repetitions; the final six-run comparison above was started afresh afterward.

### Performance guard and power mode

Independent verification initially failed the unchanged transcript guard at
84.3 ms p95 against its 67 ms limit. Five diagnostic repetitions then failed at
83–98 ms p95 with 1133–1240 ms of long tasks. `pmset` showed AC power configured
with Low Power Mode on, despite the battery profile having it off. After the
user disabled it, the identical retry-free command passed all five repetitions:
p95 was 50 ms every time and long tasks were 579–644 ms. All five archive guards
passed as well. No benchmark code, dataset, threshold or retry policy changed.
This finding prompted the new, power-controlled comparison rather than relying
on the earlier unrecorded-power speedup.
