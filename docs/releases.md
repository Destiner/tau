# Native release safety

## Current boundary

The GPUI app is canonical, but **public native distribution is not implemented**.
There is no native signed/notarized release pipeline, compatible updater feed or safe
update installation/relaunch/recovery path. The application update status is an
unavailable panel, not an updater backend.

Use `bun run package` only for a fresh local ad-hoc macOS preview after
`bun install --frozen-lockfile` and `bun run prepare`. See
[PACKAGING.md](../gpui/PACKAGING.md) for destination guards, linkage/signature checks
and separate launch safety. Preview ID `com.destiner.tau.native-preview` and version
`0.2.1` remain unchanged. A preview is not permission to install over an existing app.

Release requests use the [release skill](../.agents/skills/release/SKILL.md), which
blocks publication until native distribution is implemented and reviewed. Do not
resurrect the removed automatic Tauri publication procedure or advertise the old
v0.2.1 DMG as a native build.

## Required before public distribution

- Define supported architectures/OS, bundle identity and installation/cutover policy.
- Prepare pinned dependencies, run canonical tooling/core/native checks and build the
  optimized application. Hosted static checks alone do not prove Metal or OS behavior.
- Implement verified native artifacts, Developer ID signing, applicable notarization
  and stapling, immutable provenance/checksums and recovery from partial publication.
- Define external Pi/Node/SSH discovery requirements; these runtimes and provider
  configuration are not included in the preview bundle.
- Before enabling updates, implement compatible feed/version/platform selection,
  pinned trust, authenticated verification, safe installation/relaunch and rollback.
- Obtain explicit approval of the exact version, commit, assets and notes before any
  future publication. A request to prepare a release is not publication approval.

Manual workflow/resumption and SSH acceptance for the cold-turkey cutover does not
certify native distribution. Deferred compaction, automated busy-close and profile
campaigns remain separate scope decisions; this is not a new general acceptance matrix.

## Historical updater trust and key custody

Existing public Tauri clients and their updater trust remain a historical obligation,
not a native update implementation. Preserve historical tags/assets and trust material;
do not overwrite a published release, retarget its tag or reuse its version. Do not
redirect an old feed to incompatible GPUI payloads.

The historical updater public-key fingerprint is retained in
`scripts/updater-public-key.sha256`. A public key is not secret, but its pinned trust
must not be casually replaced. Preserve private-key backups and passwords outside
the repository and chat. Losing/rotating the old private key can strand installed
clients; a transition requires a separately authorized compatibility plan, not just
a new fingerprint. No new key generation or rotation is part of this cleanup.

Apple Developer ID certificate exports need their private keys; a `.cer` alone is
not a signing backup. Keep signing identities, password-protected exports and any
notarization API private keys in secure custody. Never print credentials, put them
in source or pass them as visible command-line arguments. These custody cautions do
not imply that native signing/notarization automation exists.

If historical publication is incomplete, inspect exact tag/commit/asset provenance
before proposing recovery. API failure is not evidence of absence. Deleting a draft
or tag needs explicit authorization; published releases must not be rewritten.
