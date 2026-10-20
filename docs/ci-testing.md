# CI and test execution

## Canonical gate

The native/tooling/core CI reports the aggregate gate **check**. It covers frozen
Bun installation, pinned native dependency preparation, tooling lint/typecheck,
Rust formatting/Clippy and deterministic tests for shared crates and the GPUI app.
Vue, Tauri and Playwright lanes are no longer product gates. The `tooling` lane
runs lint, TypeScript and Bun tests on Ubuntu; the `native` lane prepares dependencies,
runs Rust formatting/Clippy, macOS fake-process packaging and pure Swift helper
checks, shared crate tests, native library/editor tests and the optimized build on macOS. The aggregate `check` requires both lanes. Installs use
`--ignore-scripts` so only the native lane explicitly prepares patched Rust sources.

The repository-root workflow is:

```sh
bun install --frozen-lockfile
bun run prepare
bun run check
bun run test
bun run build
```

`check` is static analysis; `test` runs Linux-compatible Bun tooling/policy tests,
macOS packaging fakes, pure Swift helper checks, shared Rust suites, GPUI
library/editor integrations and Metal regressions. The optimized native
build is a separate validation step. Direct Cargo commands work after prepare with
`--manifest-path gpui/Cargo.toml`; shared crate manifests remain independently usable.

## Packaging gates

- `bun run test:tooling`: existing Linux-compatible suites plus native-package
  workflow/preflight policy, dependency-notice collection and fake synthetic-Keychain-smoke tests.
- `bun run test:packaging`: macos-bundle and macos-release fake-process suites.
  These use the actual Darwin filesystem/exclusive publication primitive but do
  not invoke live Keychain, signing/notary, Pi or GUI operations.
- `bun run test:keychain`: compiles/runs pure Swift helper checks, with **no
  live Keychain APIs**. This is not evidence that legacy file-Keychain/SPI behavior
  works on a real runner.

The dispatch-only `native-package.yml` candidate workflow is separate from the
aggregate CI gate. After approved reviewed merge/dispatch it runs generated-P12
synthetic Keychain smoke on disposable hosted CI before the protected `release`
environment's real Apple step. No signed candidate/runtime acceptance has completed.
See [release safety](releases.md) for exact inputs, provenance and approval boundaries.
Neither policy tests nor metadata audits substitute for actual Apple validation or
separately authorized final-artifact launch/install smoke.

For explicitly requested signing-helper investigation, `ci.yml` has a manual-only
`keychain_smoke` input. It runs the pure helper tests and real generated-P12 smoke
on disposable hosted macOS, without production credentials or the `release`
environment. Its fresh, marked `0700` parent is under the runner account's real
`~/Library/Keychains/tau-ci-<run>-<attempt>`; existing/default keychains are untouched,
and neither a fake home nor a fake Library/Keychains directory is used. It skips the ordinary lanes and aggregate `check`, uses separate
concurrency, and cannot satisfy the candidate's successful **push/main** CI gate:

```sh
gh workflow run ci.yml --ref REVIEW_BRANCH -f keychain_smoke=true
```

Smoke failures report fixed phases, bounded exit statuses and exact allowlisted
helper errors only. Raw subprocess output, credential input and filesystem errors
are never printed. Cleanup errors are reported separately from the initial failure;
unknown or incompletely cleaned Keychain directories remain preserved. This mode
does not build, notarize, launch, install or publish Tau.

## Metal and hosted runners

Offscreen render tests use actual Metal, not a fake renderer. They test painted
content, geometry and control event handling, not the full AppKit/OS input path.
A working Metal device and Xcode/Metal tools are required. Hosted macOS runner GPU
availability is a separate infrastructure constraint: a static/core CI pass is not
Metal evidence. Hosted CI does **not** execute the five paint targets; run
`bun run test:metal` on a Mac with an available Metal device before handoff. This
command fails on an unavailable renderer rather than skipping the assertions.
The workflow summary records that coverage boundary. Do not infer hosted GPU
support from a local capture.

## Explicit acceptance only

Default tests do not require installed Pi, provider credentials, model calls, SSH,
a visible native window or cross-app automation. Native OS targets are gated by
`native-os-tests` and selected explicitly. Installed-Pi compatibility is an opt-in
canary (`bun run test:pi-contract`), not a default dependency. See
[reproduction](reproduction.md) and [application boundaries](../gpui/APP.md).

The user's manual workflow/resumption and SSH checks are accepted for the cutover.
Additional compaction, automated busy-close and existing-profile campaigns are
deferred. Manual acceptance does not convert earlier automated failures into passes.
Do not reopen a broad campaign without a concrete regression.

## Regression and timing discipline

Test product behavior at the owning layer: shared process/storage contracts in core,
protocol ownership in session, and visible geometry/focus/drafts/selection in actual
native controls. Lower-layer tests do not replace rendered assertions. Keep durable
native regressions when removing migration-only comparisons.

Use deterministic fakes, exact request/identity fences and controlled clocks. Require
the minimized case to fail before the fix and pass afterward. Do not hide races with
arbitrary sleeps, retries, activation repair or widened deadlines.

Compare timings on the same otherwise idle host, power settings and dataset. Record
cache warmth, runner hardware and contention; on macOS, AC power alone does not mean
Low Power Mode is off. Do not compare a failing short run with a successful full run,
or present old suite counts as current verification.
