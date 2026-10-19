# Tau macOS preview packaging

Packaging builds the canonical `tau` application, not a showcase. It creates
**Tau.app**, bundle ID **com.destiner.tau.native-preview**, with the aligned
root/native version **0.3.0**.

These are local ad-hoc previews, not Developer ID signed, notarized, stapled or
updater artifacts. Separate production candidate infrastructure is implemented/in
review, but no signed candidate or runtime acceptance has completed. See
[release safety](../docs/releases.md) for the protected candidate flow and public
release blockers. The historical Tauri DMG/feed is not a native distribution path.

## Package from root

On macOS, install Bun, Cargo and Xcode/Metal tools, then:

```sh
bun install --frozen-lockfile
bun run prepare
bun run package
```

The default is an optimized normal-feature `cargo build --locked --bin tau`, with
output `gpui/target/bundle/macos/Tau.app`. The packager uses normal `CARGO_HOME`,
`gpui/target` and two build jobs. Preparation must precede packaging. It never
installs, accesses production data, opens Finder or launches the application.

For an explicit fresh debug destination:

```sh
preview=$(mktemp -d /tmp/tau-native-bundle-XXXXXX)
bun gpui/scripts/package.ts --debug --output "$preview/Tau.app"
```

`--no-build` is only an explicit opt-in for a previously built binary; it is not
fresh-build validation. Use a fresh destination each time. `--output` must be
absolute and end in `Tau.app`. Existing outputs, symlinks, normalized system/user
Applications paths and their symlink aliases are refused. Nothing is recursively
replaced. Uniquely owned staging is inspected and ad-hoc signed before an atomic
same-filesystem rename; failure cleans only that invocation's staging.

The bundle contains `Contents/MacOS/tau`, Info.plist, the existing Tau ICNS icon and
an ownership/version/profile marker at `Contents/Resources/tau-native-preview.json`.
Runtime Version and bundle metadata use the GPUI Cargo package version, aligned with
root package metadata and its lock entry. UI icons and Inter font are embedded;
no source tree or separately prepared runtime asset directory is needed. Preserve
font, icon, syntax and diagram license/source notices.

Inspection verifies Mach-O executable type, architecture, Info.plist syntax and
`codesign --verify --deep --strict`. Libraries and `LC_RPATH` must be system-only
(`/System/Library/` or `/usr/lib/`); non-system dependencies fail closed. The signature
is ad-hoc (`codesign --sign -`), not Gatekeeper distribution approval. The app declares
macOS 15+ and builds for the host architecture, not universal.

## Launch is a separate action

Double-clicking uses **production storage**. A distinct bundle ID is not isolation.
The cold-turkey product switch requires the old app closed; never run both against
the same profile. Cross-process profile locking is not certified and old Vue drafts
are not guaranteed to migrate. Do not replace `/Applications/Tau.app` with packaging;
keep previews outside system/user Applications directories.

For intentionally isolated manual use, launch the packaged executable with an explicit
profile and disposable project:

```sh
sandbox=$(mktemp -d /tmp/tau-native-profile-XXXXXX)
mkdir "$sandbox/project"
"$preview/Tau.app/Contents/MacOS/tau" \
  --isolated "$sandbox/profile" --project "$sandbox/project"
```

Pi must be separately discoverable by the runtime. The bundle does not include Pi,
Node, SSH or provider configuration. Isolation is not an OS sandbox and does not
isolate credentials, extension execution or remote sessions. Do not run GUI acceptance
concurrently with another owner's native tests.

## Production candidates and updater boundary

`bun run package:macos` prepares unsigned production input for `com.destiner.tau`,
arm64/macOS 15+; it does not change this preview command. `bun run verify:macos`
inspects final signed candidates without launching or installing them. Exact
arguments, canonical fresh-path rules, protected workflow approvals, legal payload
and remaining public-release requirements live in [release safety](../docs/releases.md).
Do not dispatch or access Apple credentials merely to build a preview.

The updater is **not implemented**. Check for Updates… / Show Update Status open the
unavailable Version popup with its primary action disabled; they do not fetch or
mutate updates. A future native updater needs a compatible feed, pinned trust,
authenticated artifact verification, safe install/relaunch, failure recovery and
rollback. Never point it at Tauri payloads or replace historical updater assets.
See [release safety](../docs/releases.md) and the [release skill](../.agents/skills/release/SKILL.md).

User manual workflow/resumption and SSH checks are accepted for the cutover;
additional compaction, existing-profile and automated busy-close campaigns are
deferred, not passing automated evidence. See [APP.md](APP.md). These scope decisions
neither certify public distribution nor reopen broad acceptance or media work.

## Static packaging regressions

```sh
bun test ./gpui/tests/package.bun.ts ./gpui/tests/version.bun.ts
```

They cover metadata, fresh-only destinations and mocked inspection commands without
launching a GUI. They do not prove notarization, installation or OS acceptance.
