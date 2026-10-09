# Tau Native macOS preview packaging

This builds the real `src/bin/tau.rs` application (`app::run`, shared `tau-core`
and `tau-session`), not a showcase or prototype launcher. It produces a standalone
**Tau Native.app**, bundle ID `com.destiner.tau.native-preview`, separate from
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
`gpui/target/bundle/macos/Tau Native.app` (generated files stay under the existing
ignored target directory). Uses normal `CARGO_HOME`, sets `CARGO_BUILD_JOBS=2`,
and explicitly uses `gpui/target`. Does not run prepare, copy sources, rebuild
caches, invoke Tauri, install anything, access production data, open Finder or
launch a GUI. Cargo's ordinary incremental compilation is still allowed.
Required patched dependencies must already be present in the development setup.

For bounded debug validation using the existing debug target:

```sh
preview=$(mktemp -d /tmp/tau-gpui-native-bundle-XXXXXX)
bun gpui/scripts/package.ts --debug --output "$preview/Tau Native.app"
```

`--no-build` is an explicit opt-in for a previously built binary:

```sh
bun gpui/scripts/package.ts --debug --no-build --output "$preview/Tau Native.app"
```

Use a fresh output directory for each invocation. `--output` must be absolute
and end in `Tau Native.app`. Existing outputs, including marked previews and
symlinks, are refused; nothing is recursively replaced. Staging is uniquely
owned, checked and ad-hoc signed before a same-filesystem rename publishes the
app. Failures clean only that invocation's staging directory. A marker in
`Contents/Resources/tau-native-preview.json` records ownership/version/profile.

The bundle contains `Contents/MacOS/tau`, an Info.plist and the existing Tau ICNS
icon. Version comes from the GPUI Cargo package; copyright metadata labels the
build as a GPUI debug/release preview. Shell icons and Inter font are compiled
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
"$preview/Tau Native.app/Contents/MacOS/tau" \
  --isolated "$sandbox/profile" --project "$sandbox/project"
```

Pi must be on PATH (or provided through the shell's configured runtime setting).
The bundle is self-contained for its native UI assets/linkage, not a bundled Pi,
Node, SSH or model-provider runtime. Isolation separates Tau storage and Pi
session files; it is not an OS sandbox and does not isolate provider credentials
or extension execution. Use only trusted runtime configuration. Never replace
`/Applications/Tau.app`; if manually installing a local preview, retain the
**Tau Native.app** name and continue launching with explicit isolation arguments.
Do not run GUI smoke tests concurrently with another owner's native tests.

## Static regression tests

```sh
bun test ./gpui/tests/package.bun.ts
```

These test paths, metadata, fresh-only destination handling and mocked inspection
commands without any OS GUI. See [APP.md](APP.md) for the separately opted-in
native application acceptance target.
