# Shared native test helpers

Deterministic Pi wire fixtures for the native Rust process tests; no browser or UI controller adapter.

## Commands

- `bun run test:tooling` - Linux-compatible tooling and native-package policy tests; no live Apple processes.
- `bun run test:packaging` - macOS bundle/release fake-process suites using actual Darwin filesystem publication; no Keychain, Pi or GUI.
- `bun run test:keychain` - Compile/run 49 pure Swift helper checks; no live Keychain APIs.

- `bun build scripts/pi-stdio-adapter.ts scripts/pi-contract.ts --target=bun --outdir=/tmp/tau-pi-test-build` - Check the standalone helper bundles without launching Pi.
- `cargo test --manifest-path crates/tau-core/Cargo.toml` - Run shared service tests, including the Bun-backed stdio fake.
- `cargo test --manifest-path crates/tau-session/Cargo.toml` - Run deterministic session-controller tests.
- `cargo test --manifest-path crates/tau-observability/Cargo.toml` - Run shared diagnostics tests.

## Structure

- `support/pi-scenario/index.ts` - Test-owned wire schema, strict request matching, captured IDs, runtime bindings, gates and timeline diagnostics; no production data-model imports.
- `support/pi-scenario/fixtures.ts` - Synthetic bootstrap state and replies.
- `support/pi-scenario/saved-session-conversation.ts` - Stdio round-trip fixture consumed by `crates/tau-core/src/pi/tests/transport.rs`.
- `support/pi-scenario/catalogue.ts` - Only scenarios consumed by retained native tests.
- `tooling/native.bun.ts` - Root command safety and native validation-target coverage; runs without launching the application.
- `tooling/native-package.bun.ts` - Workflow/preflight policy and fake synthetic-smoke process checks; Linux-compatible.
- `tooling/macos-{bundle,release}.bun.ts` - Injected native-command fakes, integrity, fresh outputs and owned cleanup regressions on Darwin.
- `../scripts/macos/signing-keychain-tests.swift` - Pure input/path/ownership/ACL-format checks, not live signing acceptance.
- `../scripts/pi-stdio-adapter.ts` - Fake Pi executable; requires Bun on PATH even when invoked by Rust.
- `../scripts/pi-contract.ts` - Opt-in read-only installed-Pi RPC canary in temporary offline storage; never part of default tests.

## Patterns

- Preserve native late-output, generation, cleanup and ownership regressions under `crates/tau-core/src/pi/tests/`; browser scenario removal does not supersede them.
- Keep scenarios deterministic and private-data-free. Match meaningful request fields and capture generated request IDs for correlated replies.
- Keep this schema test-owned; do not create a second production runtime or import host UI models.
- Prefer fake-based regressions at the narrowest layer. Installed-Pi checks (including ignored Rust canaries) require explicit opt-in; never use credentials or model calls for routine validation.
