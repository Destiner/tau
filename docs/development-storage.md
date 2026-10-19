# Development storage

## Explicit isolated development

Run from the repository root after installing tooling and preparing dependencies:

```sh
bun run dev -- --isolated /absolute/path/to/profile --project /absolute/path/to/project
```

The development launcher requires `--isolated`; `--project` is optional. Both paths
must be absolute. The profile is persistent across launches, not a per-process
temporary fixture. It starts without synthetic projects or transcripts. Tau metadata,
feedback, telemetry and native UI preferences/drafts live under `<profile>/tau/`;
local Pi sessions live under `<profile>/pi-sessions/`. Use a new profile for disposable
experiments; do not point development at production storage.

Direct invocation after `bun run prepare` is also possible:

```sh
cargo run --manifest-path gpui/Cargo.toml --bin tau -- \
  --isolated /absolute/path/to/profile --project /absolute/path/to/project
```

`--session <registered-id-or-path>` can select an existing registered session.
Isolation is explicit application state routing, not a build-mode switch.

## Isolation limits

This is **not a tool or filesystem sandbox**. Pi retains its normal authentication,
models, settings, extensions and ability to modify the selected project. Provider
credentials, extension-owned storage and remote SSH sessions are not isolated.
An extension explicitly opening a transcript outside the profile can leave the
session directory. Use an invented project and trusted Pi configuration for testing.

Tests use independent profiles and deterministic fakes rather than changing process
`HOME` or global Pi configuration. No browser sandbox or automatic Atlas/Notes seed
is part of the native development workflow.

## Production and cutover

A normal product launch without `--isolated`, including double-clicking a preview
bundle, uses the existing application-data `tau/` profile and ordinary Pi session
locations. The distinct preview bundle ID does **not** isolate storage.

The authorized switch is cold-turkey: close the old Vue/Tauri app before launching
the native app. Never run both as concurrent writers to the same profile;
cross-process profile locking is not certified. There is no destructive transcript
conversion: Pi owns its files, while native drafts/preferences use `native-ui.json`.
Migration of old Vue drafts is not guaranteed; preserve important text separately
before switching. Do not delete legacy state or sessions as part of repository cleanup.

An additional existing-profile acceptance campaign is deferred, not a claimed pass
or a prerequisite to reopen without a concrete regression. See [APP.md](../gpui/APP.md)
for the accepted manual and deferred automated checks.
