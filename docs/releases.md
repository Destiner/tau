# macOS release infrastructure

For versioning, checks, workflow dispatch, draft notes, publication approval, and post-publication verification, use the [release skill](../.agents/skills/release/SKILL.md). This document covers infrastructure setup, local builds, and recovery—not a second release procedure. The implementation lives in `.github/workflows/release.yml`, `scripts/release-preflight.sh`, `scripts/release-macos.ts`, and `scripts/updater-release.ts`.

Tau distributes an Apple Silicon DMG for macOS 15+, signed with Developer ID, notarized, and stapled. Its `macos-private-api` feature suppresses WKWebView's white first paint; this private API rules out Mac App Store submission. Routine `bun tauri build` produces only an unsigned `.app`, avoiding the DMG packager's Finder AppleScript. Use `bun tauri build --bundles app,dmg` only to test packaging, or `bun run release:macos` for distribution.

## Apple setup

Paid [Apple Developer Program](https://developer.apple.com/programs/enroll/) membership is required. The team's Account Holder creates a **Developer ID Application** certificate using **G2 Sub-CA** in [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/certificates/list). Follow [Apple's CSR and certificate instructions](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/), install the certificate in the login keychain, and confirm `security find-identity -v -p codesigning` lists the identity.

For CI, export the identity **with its private key** from Keychain Access → My Certificates as a password-protected `.p12`. A `.cer` alone is insufficient; a missing private key must come from the originating Mac or a new certificate. Encode the export with `base64 -i /path/to/DeveloperID.p12 | pbcopy`, store it in the secret below, then clear the clipboard. Keep all credentials and certificate exports outside the repository and chat.

For notarization, an Account Holder or Admin creates a **team API key** with Developer role in [App Store Connect → Users and Access → Integrations](https://appstoreconnect.apple.com/access/integrations/api). Request API access if prompted. Record the Key ID and Issuer ID; Apple permits downloading the `.p8` private key only once. An individual API key does not fit this issuer-based setup. See [Apple's API key instructions](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api).

## GitHub environment

Create the **release** environment in repository Settings → Environments. Restrict deployments to selected branch **main**, not tags; optionally require reviewers. Configure these environment secrets:

| Secret                               | Source                                                                        |
| ------------------------------------ | ----------------------------------------------------------------------------- |
| `APPLE_CERTIFICATE`                  | Base64 `.p12` containing Developer ID Application certificate and private key |
| `APPLE_CERTIFICATE_PASSWORD`         | `.p12` export password                                                        |
| `APPLE_API_PRIVATE_KEY`              | Entire downloaded `.p8`, including BEGIN/END lines                            |
| `APPLE_API_KEY`                      | App Store Connect Key ID                                                      |
| `APPLE_API_ISSUER`                   | Team API key Issuer ID                                                        |
| `TAURI_SIGNING_PRIVATE_KEY`          | Complete encoded updater private key                                          |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater-key password; unset for an unencrypted key                            |
| `TAU_UPDATER_PUBLIC_KEY`             | Complete encoded updater `.pub` content, compiled into release builds         |

Actions policy must allow the built-in `GITHUB_TOKEN` to use `contents: write`; no personal token is needed. The workflow captures `main`'s HEAD for both checkout and tagging, creates a lightweight version tag and a draft, and removes temporary signing credentials in an always-run cleanup step. Draft assets remain unavailable to installed clients until publication as the latest stable release.

## Updater trust and recovery

Generate the updater keypair **once**, not for each release:

```sh
bunx tauri signer generate -w ~/.tauri/tau-updater.key
```

Back up both files and the password securely. Losing or rotating the private key prevents existing installations from trusting updates. The public key is not secret; its lowercase SHA-256 fingerprint is pinned in `scripts/updater-public-key.sha256`. Verify a restored public key before configuring CI:

```sh
printf '%s' "$(cat ~/.tauri/tau-updater.key.pub)" \
  | base64 --decode \
  | shasum -a 256
```

Each release verifies the configured public key against that fingerprint. Rotation requires an authorized transition shipped to existing installations first; changing only the fingerprint and secret strands clients.

## Local build and verification

Install the target with `rustup target add aarch64-apple-darwin`, then provide the installed signing identity, notarization credentials, and updater key:

```sh
APPLE_API_KEY=... \
APPLE_API_ISSUER=... \
APPLE_API_KEY_PATH=/path/to/AuthKey_....p8 \
APPLE_SIGNING_IDENTITY="Developer ID Application: ..." \
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/tau-updater.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=... \
TAU_UPDATER_PUBLIC_KEY="$(cat ~/.tauri/tau-updater.key.pub)" \
bun run release:macos
```

Alternatively, notarize with `APPLE_ID`, app-specific `APPLE_PASSWORD`, and `APPLE_TEAM_ID` instead of the API variables. CI can import the identity using `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD`. Omit the updater password for an unencrypted key.

The script builds/typechecks the frontend without signing variables, then runs the credentialed native build. Updater artifacts are enabled only by `src-tauri/tauri.release.conf.json`; the public key is injected through a temporary configuration. Assets are staged atomically under `src-tauri/target/release-assets/<version>` without overwriting. Preserve or explicitly remove an existing local version directory before rebuilding.

The verifier checks the exact four assets (DMG, updater archive, signature, `latest.json`), manifest correspondence, cryptographic updater signature, safe archive entries, app/DMG equality, arm64 architecture, minimum OS, signing, notarization, Gatekeeper, system-only linkage, file allowlists, and synchronized versions. It prints SHA-256 checksums. Reverify without rebuilding:

```sh
TAU_UPDATER_PUBLIC_KEY="$(cat ~/.tauri/tau-updater.key.pub)" \
  bun run verify:macos-release
```

Append `-- /path/to/release-assets/<version>` to verify a specific asset directory. For published releases, use the release skill's unauthenticated public-manifest comparison and immutable archive URL check against retained, checksum-verified draft assets—not a guessed local build directory.

## Failed or partial releases

Concurrency is not FIFO: a new dispatch can replace a pending run. Inspect existing runs before retrying. Existing tags/releases (including drafts) and API errors stop preflight; never move tags or overwrite assets. `--verify-tag` proves existence, not commit identity; verify the exact SHA, and use tag protection for downstream immutability. Tags pushed by `GITHUB_TOKEN` do not trigger tag/push workflows.

Before tagging, fix the failure and rerun. After tagging, runner artifacts are not retained: inspect the tag and partial draft, and obtain explicit permission before deleting either. Complete manually only with the exact verified assets already uploaded or independently retained. Never delete or retarget a published release to reuse its version; bump instead. Notarization delays may require retry.

## Optional distribution smoke test

This is optional, not a publication gate. Download the exact verified DMG over HTTPS in Safari on a clean test Mac, compare its SHA-256, drag Tau into Applications, and open it without `xattr`, `chmod`, or Gatekeeper overrides. Run `spctl --assess --type execute --verbose=4 /Applications/Tau.app`; expect `accepted` and `source=Notarized Developer ID`.

Do not use Telegram for this check: its App Sandbox quarantine metadata can cause `File created by an AppSandbox, exec/open not allowed` even for a valid app.
