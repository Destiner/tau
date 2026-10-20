# Native release safety

## Current boundary

Native packaging infrastructure and signing-helper corrections are merged. Hosted
synthetic acceptance now passes. The next candidate built and verified its unsigned
input, but protected certificate import failed; owned cleanup succeeded. The specific
helper failure remains unknown, and redacted import diagnostics require review before
another attempt. Production identity **com.destiner.tau**, native version **0.3.0**,
and use of the existing protected main-only GitHub `release` environment were
explicitly chosen. **No signed candidate or packaged runtime acceptance has
been completed.** Implementation approval is not approval to merge, dispatch,
publish or replace an installed app.

`bun run package` remains a local ad-hoc preview: ID
`com.destiner.tau.native-preview`, version aligned with root/native metadata (0.3.0).
See [PACKAGING.md](../gpui/PACKAGING.md) for preview details and storage safety.
The historical v0.2.1 DMG is Vue/Tauri, not native Tau.

## Candidate flow after reviewed merge

Read the [release skill](../.agents/skills/release/SKILL.md) and actual
[workflow](../.github/workflows/native-package.yml) before an authorized run.
Do not relax the existing main-only environment. Its branch restriction does not
replace the separate merge and dispatch approvals below.

1. Obtain separate approval for the reviewed merge and then dispatch. Require the
   exact current main commit, aligned root/native/lock version and successful
   push/main CI from `ci.yml`, including its aggregate `check` job. Public `vVERSION`
   tag must be absent; API errors, pending CI and mismatched provenance fail closed.
2. Dispatch `native-package.yml` on `main` with required inputs `commit` (actual
   lowercase 40-character main SHA) and `version` (`0.3.0`). For example, **only
   after dispatch approval**, with `COMMIT` set to that reviewed SHA:

   ```sh
   gh workflow run native-package.yml --ref main -f commit="$COMMIT" -f version=0.3.0
   ```

3. Check the exact run's inputs, head SHA and both job results, not merely a check
   name. The credential-free build job runs fake-process packaging tests, pure
   Swift checks and generated-P12 synthetic Keychain smoke on a disposable hosted
   macOS 15 arm64 runner before building the normal-feature optimized app.
4. The separate `release`-environment job rechecks CI/tag provenance, verifies the
   same-run unsigned artifact ID and tar digest before extraction, and inspects
   input before the protected signing step binds existing Apple secrets. Updater
   secrets are untouched. Workflow permissions are only contents/read and
   actions/read; there are no release writes.
5. Inspect the exact final artifact and manifest. Final output is only
   `tau-0.3.0-apple-silicon.dmg`, `manifest.json` and `SHA256SUMS`. The run summary
   identifies source, artifact URL/ID and notarization boundaries. Workflow
   artifacts in this **public repository may be downloadable**; they are not
   private storage or GitHub releases.

The helper uses deprecated file-Keychain APIs and explicitly resolved
`SecKeychainItemSetAccessWithPassword` SPI. Compilation and pure Swift checks do not
validate live ACL/import behavior. Synthetic hosted smoke must succeed before real
Apple credentials are used; actual signing/notarization behavior remains unvalidated.
There is no permissive fallback or credential-in-argv workaround.

The disposable runner creates a fresh `0700`, marked parent beneath its account's
real `~/Library/Keychains/tau-ci-<run>-<attempt>`. Only synthetic smoke and temporary
signing work use this reserved namespace; compilation, downloads and final artifacts
remain in `RUNNER_TEMP`. The existing Keychains directory must already be canonical,
safely permissioned and account-owned. Never override `HOME`, create a lookalike
`Library/Keychains`, move the login keychain or modify existing/default keychains.
The signing command and always-cleanup use the same exact owned work path.

Location matters: Security.framework creates legacy-format databases outside its
Keychains location, without the required partition ACL. The helper checks the
actual database format before import, keeps exactly-one partition enforcement and
uses the legacy API's `INT_MAX` no-timeout sentinel (zero locks immediately). Its
owned inventory includes only the exact empty AtomicFile lock companion, not a
wildcard allowance. After validated import, signing explicitly activates only the
owned keychain: it appends that exact path to the user-domain search list, preserving
all existing ordered entries and requiring exact readback. The merged effective
list is not used for new snapshots; it includes common/dynamic entries that the
legacy setter can reposition. The default keychain and trust stores are never set.

The active ownership marker precedes this temporary registration. Cleanup removes
only the owned entry from the current user list before publication, retaining
recovery on failure. Older merged-list recovery snapshots remain readable and use
the legacy restoration operation rather than being misinterpreted as user-domain
snapshots. Synthetic acceptance verifies import, noninteractive signing, removal,
wrong-password rollback and restoration of the original user search list/default.
The default is also checked while the owned signing entry is active.

## Artifact verification and local commands

On macOS arm64 after frozen install and native dependency preparation, unsigned
preparation requires an exact clean commit, aligned version and fresh absolute
output. These examples are instructions, not authorization to build or mount:

```sh
# COMMIT must be the actual reviewed 40-character SHA, not a placeholder hash.
candidate_root=$(mktemp -d /private/tmp/tau-native-candidate-XXXXXX)
bun run package:macos -- --version 0.3.0 --commit "$COMMIT" \
  --output "$candidate_root/unsigned"
# FINAL_DIR is the canonical directory containing the three final workflow files.
bun run verify:macos -- --version 0.3.0 --commit "$COMMIT" \
  --input "$FINAL_DIR" --work "$candidate_root/verify-work"
```

Production paths must be canonical: use `/private/tmp`, not `/tmp` aliases.
Output/work directories must be fresh, nonoverlapping and outside Applications;
never overwrite artifacts. `package:macos` is `prepare`, not signing. `verify:macos`
performs credential-free final inspection, including a readonly mount, never an app
launch or installation. It still requires native Apple tooling/assessment.

Unsigned verification checks regular files, complete file hashes, plist/provenance,
arm64, a Mach-O deployment target no newer than macOS 15.0, and system-only dylibs/rpaths.
Production builds use two Cargo jobs and explicitly target macOS 15.0. Signing requires Developer ID, hardened runtime,
secure timestamp and no entitlements. The app is notarized and stapled **before** DMG
creation; the DMG is separately signed, notarized and stapled. Final inspection
checks signatures, tickets, Gatekeeper and the readonly mounted app/layout.
Pending, rejected or failed notarization is failure, not success.

Cleanup touches only marker-owned paths. Pending/ambiguous mounts are preserved
until exact owned detach succeeds; never use `rm -rf` or global/forced unmount as a
fallback. Runner death can prevent cleanup. Hash manifests are integrity inventories,
not independent authentication: trust also depends on reviewed source and same-run
artifact transfer provenance. Metadata/signature inspection is not runtime acceptance.

`Contents/Resources/Legal` preserves Tau/asset/diagram notices and license inventory,
patches, native Cargo/preparation inputs and copied prepared `merman-render` and
`merman-layout-elk` sources, including modified EPL source. `SOURCE-OFFER.txt` points
to bundled sources/patches and upstream versioned references/checksums. Preserve EPL,
MPL and other source/notice obligations. `Legal/RustDependencies` also inventories the
locked Apple Silicon normal/build dependency graph, with deduplicated notice texts,
source/checksum references and factual author metadata. Where archives omit notices,
[provenance-pinned supplements](../gpui/assets/dependency-licenses/README.md) retain
upstream summaries and declared standard terms without inventing copyright holders.
These recorded evidence gaps are not legal certification or a guarantee that upstream
provided every notice; review obligations before public distribution.

## Public release blockers

Before publication, require reviewed merge, a successful candidate with actual Apple
validation, and a narrowly scoped, explicitly authorized final-artifact launch/install
smoke. Resolve external Pi/Node/SSH discovery requirements (none are bundled), install
cutover/recovery policy and the update strategy. **A manual-update public first release
has not been approved.** The native updater/feed/install/relaunch/rollback path remains
out of scope and unimplemented; the UI shows unavailable status.

Obtain explicit approval of the exact version, commit, assets and notes before any
publication. Candidate generation grants no permission for a tag, draft, GitHub
release, latest feed, README download link or installed-app replacement. Hosted CI
still does not prove Metal paint; local Metal verification remains separate. Existing
manual workflow/resumption and SSH acceptance does not certify distribution. Do not
reopen broad parity, media, compaction, busy-close or profile campaigns for this work.

## Historical trust and custody

Preserve historical Tauri tags/assets and updater trust; never retarget tags,
overwrite published assets, reuse a published version or redirect the old feed to
GPUI payloads. The pinned historical public-key fingerprint remains in
`scripts/updater-public-key.sha256`; updater keys are not part of native packaging.
API failure is not evidence of absence. Draft/tag deletion requires explicit approval.

Keep Apple certificate exports **with private keys**, passwords and API private keys
in secure custody outside source/chat. Reuse the existing Apple secrets only in the
protected signing step; never print secrets, put them in argv or expose credential
tracing. Key rotation or historical updater recovery requires separate authorization.
