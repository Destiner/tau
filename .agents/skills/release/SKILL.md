---
name: release
description: Handles explicit Tau preview and protected native candidate packaging requests; preserves approval gates and blocks public publication until release requirements are resolved.
disable-model-invocation: true
---

# Release Tau

Use only when explicitly invoked. Editing this skill is not authorization to build,
merge, dispatch, push, tag, publish, install or launch anything.

## Establish scope

Read `AGENTS.md`, `docs/releases.md`, `gpui/PACKAGING.md` and actual scripts/workflow
before acting. Production ID `com.destiner.tau` and version `0.3.0` are authorized;
packaging infrastructure is implemented/in review, with **no completed signed
candidate or packaged runtime acceptance**. The native updater is out of scope.
Manual updates for a public first release remain undecided, not approved.

Distinguish a local preview, protected candidate and public release. Implementation
approval does not authorize PR merge or workflow dispatch; obtain those separately.
Never relax the existing protected main-only `release` environment. Do not replace
the installed app, read credentials or launch a candidate without explicit approval.

## Local ad-hoc preview

For an explicitly requested preview, inspect working-tree state without discarding
unrelated work. Use frozen install, `bun run prepare`, `bun run check`, `bun run test`
and `bun run build` as applicable, then `bun run package`. Report unavailable/failed
checks; do not present stale output as fresh verification.

Keep preview ID `com.destiner.tau.native-preview`; its version tracks aligned
root/native metadata (0.3.0), not historical 0.2.1. Follow `gpui/PACKAGING.md` for a
fresh destination if default output exists. Never overwrite bundles or use
Applications paths. Return artifact path, source revision/dirty-tree caveats and
checks performed. Label it **local ad-hoc preview — not notarized or publicly
distributable**.

Packaging does not authorize GUI launch or installation. Double-click uses shared
production storage; isolated manual use needs `--isolated`, which is not a tool or
credential sandbox. Never run old/native apps against the same profile simultaneously.

## Protected candidate

Follow `docs/releases.md`, which owns the procedure and exact command examples.
After separately approved reviewed merge and dispatch, use `native-package.yml`
on `main` with `commit` equal to the actual current main 40-character lowercase SHA
and `version=0.3.0`. Require exact `ci.yml` push/main success and aggregate `check`,
aligned versions and public tag absence. API errors/pending checks are not success.

The workflow separates credential-free build/synthetic generated-P12 smoke from
protected real Apple signing. Reuse existing Apple secrets only there; updater
secrets remain untouched. Never print secrets, pass them in argv or enable tracing.
The helper's deprecated file-Keychain APIs and explicit
`SecKeychainItemSetAccessWithPassword` SPI still require real hosted validation.
Pure Swift/fake-process passes are not live Keychain or signing proof.

`package:macos` prepares unsigned input, while `verify:macos` inspects final output,
including readonly mounting, without launch/install. Use fresh canonical paths
(`/private/tmp`, not `/tmp`), preserve pending mounts and unknown ownership, and
never replace failed cleanup with recursive deletion or global unmount.

Check the exact run SHA/inputs/results and artifact ID, manifest, hashes and both
Accepted notarizations. App stapling precedes DMG creation; DMG notarization/stapling
is separate. Report final DMG, manifest and SHA256SUMS provenance and caveats.
Public-repository workflow artifacts may be downloadable; they are not private or
GitHub releases. Preserve bundled legal notices and modified-source payload; do not
claim whole-dependency legal certification.

## Publication remains blocked

Require successful actual Apple candidate validation, narrow explicitly authorized
final-artifact launch/install smoke, external Pi/Node/SSH requirements, install
cutover/recovery and update/manual-release decision, then explicit approval of the
exact version, commit, assets and notes. Do not reopen broad parity, media,
compaction, busy-close or profile campaigns. Hosted CI is not Metal paint evidence.

Until those gates are resolved, do not create tags/drafts/releases, publish assets
or latest feeds, change README download links or replace installed apps. Candidate
generation is not publication permission. Preserve historical v0.2.1 Tauri assets,
tags and updater trust; never advertise them as native or redirect their feed to
GPUI payloads. Key rotation and historical recovery need separate approval.
