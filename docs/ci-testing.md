# CI and test execution

## Canonical gate

The native/tooling/core CI reports the aggregate gate **check**. It covers frozen
Bun installation, pinned native dependency preparation, tooling lint/typecheck,
Rust formatting/Clippy and deterministic tests for shared crates and the GPUI app.
Vue, Tauri and Playwright lanes are no longer product gates. The `tooling` lane
runs lint, TypeScript and Bun tests on Ubuntu; the `native` lane prepares dependencies,
runs Rust formatting/Clippy, shared crate tests, native library/editor tests and the
optimized build on macOS. The aggregate `check` requires both lanes. Installs use
`--ignore-scripts` so only the native lane explicitly prepares patched Rust sources.

The repository-root workflow is:

```sh
bun install --frozen-lockfile
bun run prepare
bun run check
bun run test
bun run build
```

`check` is static analysis; `test` runs deterministic Bun tooling tests, shared Rust
suites, GPUI library/editor integrations and Metal regressions. The optimized native
build is a separate validation step. Direct Cargo commands work after prepare with
`--manifest-path gpui/Cargo.toml`; shared crate manifests remain independently usable.

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
