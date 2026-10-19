---
name: release
description: Handles explicit Tau release or packaging requests safely. Public native releases are blocked until native distribution is implemented; currently guides fresh local ad-hoc preview packaging only.
disable-model-invocation: true
---

# Release Tau

Use only when explicitly invoked. Editing this skill is not authorization to build,
release, push, tag, publish, install or launch anything.

## Public release is blocked

Read `AGENTS.md`, `docs/releases.md` and `gpui/PACKAGING.md` first. The GPUI app is
canonical, but native distribution and the updater are not implemented. Explain
this boundary and stop public-release preparation/publication. Do not bump versions,
commit, push, create a tag/draft, dispatch a publication workflow or upload assets.
Do not revive old Tauri release commands, publish its feed, or present its v0.2.1
DMG as a native artifact. A general release request does not authorize implementing
missing release infrastructure.

Public release can proceed only after a separately authorized implementation and
review establish native signing/notarization, verified artifacts, installation and
recovery policy, and compatible updater trust if updates are enabled. This skill
must then be updated to the reviewed implementation; do not invent a procedure.
Future publication must still require approval of the exact version, commit, assets
and release notes. Preserve historical releases and updater trust/key custody as
described in `docs/releases.md`.

## Local preview alternative

If the user explicitly wants an ad-hoc preview instead, inspect current scripts and
working-tree state without committing or discarding unrelated changes. Keep version
`0.2.1` and preview ID `com.destiner.tau.native-preview` unchanged. Use the canonical
root tooling:

```sh
bun install --frozen-lockfile
bun run prepare
bun run check
bun run test
bun run build
bun run package
```

Report failed/unavailable checks; do not claim a verified preview from stale output.
Packaging must use a fresh destination and refuse existing bundles, symlinks and
system/user Applications paths. If default output exists, follow `gpui/PACKAGING.md`
for an explicit fresh output path; never remove/overwrite it automatically.

Return the artifact path, source revision/working-tree caveats and checks performed.
Label it **local ad-hoc preview — not notarized or publicly distributable**.
Packaging does not authorize installation, Finder/GUI launch, Pi/model/SSH tests,
GitHub uploads or a README download-link change. Explain that double-click uses
shared production storage; isolated manual use requires explicit `--isolated`, and
the old app must not run against the same profile simultaneously.
