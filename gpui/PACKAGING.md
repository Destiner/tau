# Tau macOS native preview packaging

This builds the real `src/bin/tau.rs` application (`app::run`, shared `tau-core`
and `tau-session`), not a showcase or prototype launcher. It produces a standalone
**Tau.app**, bundle ID `com.destiner.tau.native-preview`, with display name **Tau**, distinct in identity from
Tauri's **Tau.app** / `com.destiner.tau`. The existing Tauri fallback, release
signing, updater and installed app are untouched. Production cutover remains
**NO-GO**: these are local previews, not Developer ID signed, notarized, stapled
or updater artifacts.

## Package

Run from the repository root on macOS with Bun, Cargo and Xcode command-line tools:

```sh
bun gpui/scripts/package.ts
```

Default: release `cargo build --locked --bin tau`, output
`gpui/target/bundle/macos/Tau.app` (generated files stay under the existing
ignored target directory). Uses normal `CARGO_HOME`, sets `CARGO_BUILD_JOBS=2`,
and explicitly uses `gpui/target`. Does not run prepare, copy sources, rebuild
caches, invoke Tauri, install anything, access production data, open Finder or
launch a GUI. Cargo's ordinary incremental compilation is still allowed.
Required patched dependencies must already be present in the development setup.

For bounded debug validation using the existing debug target:

```sh
preview=$(mktemp -d /tmp/tau-gpui-native-bundle-XXXXXX)
bun gpui/scripts/package.ts --debug --output "$preview/Tau.app"
```

`--no-build` is an explicit opt-in for a previously built binary:

```sh
bun gpui/scripts/package.ts --debug --no-build --output "$preview/Tau.app"
```

Use a fresh output directory for each invocation. `--output` must be absolute
and end in `Tau.app`. System and user Applications locations are refused,
including normalized paths and symlink aliases. Existing outputs, including marked previews and
symlinks, are refused; nothing is recursively replaced. Staging is uniquely
owned, checked and ad-hoc signed before a same-filesystem rename publishes the
app. Failures clean only that invocation's staging directory. A marker in
`Contents/Resources/tau-native-preview.json` records ownership/version/profile.

The bundle contains `Contents/MacOS/tau`, an Info.plist and the existing Tau ICNS
icon. Version comes from the GPUI Cargo package, aligned with root/Tauri manifests and
its local lock entry at **0.2.1**. Runtime Version and bundle metadata use Cargo's
version, not a display override; read-only regression tests protect that alignment.
Copyright metadata labels the build as a GPUI debug/release preview. Shell icons and Inter font are compiled
into the binary (`Assets` delegates to `support::asset` / `include_bytes!`);
GPUI's embedded assets do not require copying source directories. No separately
prepared runtime asset directory is needed.

Packaging verifies Mach-O executable type, arm64/x86_64 architecture, Info.plist
syntax and `codesign --verify --deep --strict`. `otool -L` must contain only
`/System/Library/` or `/usr/lib/` dependencies; `LC_RPATH` entries must also be
system-only. Non-system libraries/rpaths fail closed rather than silently
shipping a host-dependent app. No dylib vendoring is currently needed. Signature
is **ad-hoc** (`codesign --sign -`); Gatekeeper distribution approval is not claimed.
The bundle declares macOS 15+, and builds for the host architecture, not universal.

## Safe manual launch (separate from packaging)

**Do not double-click the preview:** the actual shell defaults to production
storage without `--isolated`. A distinct bundle ID does not isolate its data.
Use a disposable project and profile; packaging intentionally never launches:

```sh
sandbox=$(mktemp -d /tmp/tau-native-profile-XXXXXX)
mkdir "$sandbox/project"
"$preview/Tau.app/Contents/MacOS/tau" \
  --isolated "$sandbox/profile" --project "$sandbox/project"
```

Pi must be on PATH (or provided through the shell's configured runtime setting).
The bundle is self-contained for its native UI assets/linkage, not a bundled Pi,
Node, SSH or model-provider runtime. Isolation separates Tau storage and Pi
session files; it is not an OS sandbox and does not isolate provider credentials
or extension execution. Use only trusted runtime configuration. Never replace
`/Applications/Tau.app`; keep previews in fresh development directories outside system/user Applications
and continue launching with explicit isolation arguments.
Do not run GUI smoke tests concurrently with another owner's native tests.

## GitHub release and updater requirements

**Yes, GitHub workflow changes are required before native distribution or updates.**
The current `.github/workflows/release.yml` builds the Vue frontend and invokes
`bun run release:macos` for Tauri. It uploads four Tauri assets from
`src-tauri/target/release-assets/$VERSION`: a DMG, updater tarball, signature and
`latest.json`. Neither that tarball nor that feed is a compatible GPUI update path.
The existing `.github/workflows/gpui-prototype.yml` is comparison/static-test CI,
triggered manually or by selected paths on the `gpui` branch; it does not publish a
native release. Its current push filters include `gpui/**` but not shared `crates/**`.

A native release workflow must prepare the pinned patched dependencies, validate the
shared controller/backend and native application, build and verify an arm64 release
bundle, and sign/notarize/staple applicable distribution artifacts. Publish separately
identified native artifacts and a compatible native feed without replacing Tauri's
fallback artifacts or directing native clients to Tauri payloads. Extend native CI
coverage/path filters to shared crates and relevant release tooling. Local preview
packaging and comparison artifact uploads do not satisfy this gate.

The runtime updater is **not implemented**: Check for Updates… and palette Show Update
Status open the Version popup with unavailable copy and a disabled primary action.
Settings is intentionally absent. These status routes do not fetch or mutate updates. Before enabling it, implement
feed/version/platform selection, authenticated artifact verification with pinned trust,
safe installation/relaunch, failure recovery and rollback. Define the native bundle
identity/cutover policy and Pi discovery/runtime requirements explicitly: the bundle
does not include Pi, Node, SSH or provider configuration. Release-profile packaged
local/SSH acceptance remains outstanding. Future signing/update key custody and
rotation are a separate release decision, not part of this parity batch.

## Current acceptance boundary

The final source passes the normal-feature native build, 691 GPUI tests / 1 ignored,
strict all-target/all-feature Clippy, static tooling and eight package/version tests.
Representative production captures and actual-label glyph controls pass, with the
selection-paint differences recorded in [APP.md](APP.md). Neither current installed-Pi
native mode passes: both hit the unchanged five-second busy-close gate with an inactive owned window,
Close unavailable and no lifecycle callback. The deactivation cause is unproven; no
activation repair was added. Historical native passes, harness cleanup receipts and
representative two-scale visual captures do not certify packaged lifecycle acceptance.
Release-profile local/SSH workflows, signing/notarization, updater trust, production
cutover, configuration and cross-process profile locking remain unverified.

## Static regression tests

```sh
bun test ./gpui/tests/package.bun.ts ./gpui/tests/version.bun.ts
```

These test paths, metadata, fresh-only destination handling and mocked inspection
commands without any OS GUI. See [APP.md](APP.md) for the separately opted-in
native application acceptance target.
