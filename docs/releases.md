# macOS releases

Tau's public DMG supports Apple Silicon Macs running macOS 15 or later. A public download must be signed with a Developer ID Application certificate, notarized by Apple, and stapled before upload. An ordinary `bun tauri build` is only suitable for local testing.

## Apple setup

Direct distribution outside the Mac App Store requires paid [Apple Developer Program](https://developer.apple.com/programs/enroll/) membership. Only the team's Account Holder can create a Developer ID certificate.

Create and install the certificate:

1. Open [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/certificates/list).
2. Add a certificate and select **Developer ID** under Software.
3. Select **Developer ID Application**, then choose **G2 Sub-CA**. The previous intermediary exists only for software signed with Xcode releases earlier than 11.4.1 and is unnecessary for Tau.
4. Create and upload the requested certificate signing request.
5. Download the `.cer` file and double-click it to install it in the login keychain.
6. Confirm that `security find-identity -v -p codesigning` lists `Developer ID Application`.

Apple's detailed process is in [Create Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/).

Notarization also needs one of:

- An App Store Connect API key: `APPLE_API_KEY`, `APPLE_API_ISSUER`, and `APPLE_API_KEY_PATH`.
- An Apple Account with an app-specific password: `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID`.

Keep certificate exports, private keys, and passwords outside the repository.

## GitHub Actions releases

`.github/workflows/release-macos.yml` is manually dispatched and accepts only `main`. It builds the exact commit selected at dispatch, not a later tip of the branch. It reads the stable `major.minor.patch` version from `package.json`, checks the Tauri and Cargo versions agree, and creates tag `v<version>` and a **draft** release titled `Tau v<version>`. The draft contains the verified Apple Silicon DMG and `SHA256SUMS.txt`, with an empty description. Nothing is automatically published.

### One-time GitHub setup

In the repository's **Settings → Environments**, create an environment named **release**:

- Restrict deployment branches to **Selected branches and tags → branch `main`** (do not allow tags).
- Optionally require reviewer approval if your GitHub plan supports it.
- Add the following **environment secrets**. Do not paste credentials into issues, chat, or the repository.

| Secret                       | Value and source                                                                                                        |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | Base64-encoded `.p12` export of your **Developer ID Application** certificate **and private key** from Keychain Access. |
| `APPLE_CERTIFICATE_PASSWORD` | Password you choose when exporting that `.p12`.                                                                         |
| `APPLE_API_PRIVATE_KEY`      | Entire contents of the downloaded `AuthKey_<key-id>.p8`, including its BEGIN/END lines.                                 |
| `APPLE_API_KEY`              | The API key's **Key ID** from App Store Connect.                                                                        |
| `APPLE_API_ISSUER`           | The **Issuer ID** shown with the team API keys in App Store Connect.                                                    |

To export the signing certificate, open **Keychain Access → My Certificates**, find the Developer ID Application identity described above, and confirm it expands to show a private key. Export the identity as `.p12` and set a strong export password. A `.cer` alone is insufficient. If the private key is missing, export from the Mac that created the certificate or create a new certificate. Copy the base64 export to the clipboard with:

```sh
base64 -i /path/to/DeveloperID.p12 | pbcopy
```

Paste into `APPLE_CERTIFICATE`, then clear the clipboard. Store the export securely outside the repository.

For notarization, an Account Holder or Admin can create a **team API key** in [App Store Connect → Users and Access → Integrations → App Store Connect API](https://appstoreconnect.apple.com/access/integrations/api). Request API access first if prompted. Create a key with the Developer role for notarization, record its Key ID and Issuer ID, and download the `.p8` file. Apple allows downloading the private key only once. Use a team key, not an individual key, for this issuer-based setup. See [Apple's API key instructions](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api).

No personal GitHub token is needed: the workflow's built-in `GITHUB_TOKEN` has `contents: write` for draft discovery, tag creation, and asset uploads. Repository/organization Actions policies must permit the workflow's actions and this permission. The workflow imports the certificate into a temporary keychain and removes it and the notarization key in an always-run cleanup step; Apple secrets are exposed only to the steps that need them.

### Creating a release

1. Bump `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` together. Refresh `src-tauri/Cargo.lock` with Cargo after changing its package version, and commit the version changes to `main`.
2. Open **Actions → Release macOS → Run workflow**, select `main`, and run it. Approve the `release` environment deployment if configured.
3. The workflow checks availability before building, runs frontend/Rust/browser checks, then signs, notarizes, staples, and verifies using `bun run release:macos`.
4. Open the resulting draft under **Releases**, write the features/fixes description, complete the distribution smoke test below, and click **Publish release** when ready.

Runs are serialized without cancelling an in-progress release. An existing release (including a draft) or an existing exact version tag stops the workflow; API failures also stop it. The workflow rechecks before signing and before creating the tag, never moves tags, and never replaces existing assets.

If a run fails before tag creation, fix the cause and rerun. If tag creation succeeds but draft creation/upload fails, inspect the tag and any partial draft manually. Finish that draft with the exact verified artifacts, or remove the incomplete draft and unpublished tag deliberately before retrying. Do not delete or retarget a published release to reuse its version: bump the version instead. Notarization service delays can also require a retry.

## Build and verify

Install the Apple Silicon Rust target in the active rustup toolchain:

```sh
rustup target add aarch64-apple-darwin
```

Using App Store Connect API credentials:

```sh
APPLE_API_KEY=... \
APPLE_API_ISSUER=... \
APPLE_API_KEY_PATH=/path/to/AuthKey_....p8 \
APPLE_SIGNING_IDENTITY="Developer ID Application: ..." \
bun run release:macos
```

Or replace the API variables with `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID`. A CI build can instead provide `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` for certificate import.

The release command builds an Apple Silicon app, notarizes and staples it before packaging, then signs, notarizes, and staples the DMG. It checks:

- exact `arm64`-only architecture coverage;
- the declared and compiled macOS deployment targets;
- app and DMG signatures, hardened runtime, Gatekeeper assessment, and notarization tickets;
- system-only dynamic library linkage;
- the complete DMG and app-bundle file allowlists;
- synchronized package versions.

It prints the artifact's SHA-256 checksum after verification.

Re-run the checks without rebuilding:

```sh
bun run verify:macos-release
```

Pass a specific DMG after `--` when needed:

```sh
bun run verify:macos-release -- /path/to/Tau.dmg
```

## Distribution smoke test

Test the exact verified DMG through a normal browser download before publishing it. Upload it unchanged to an HTTPS host, download it in Safari on a clean test Mac, confirm its printed SHA-256 checksum, drag Tau into Applications, and open it without using `xattr`, `chmod`, or Gatekeeper overrides. Confirm the installed app independently:

```sh
spctl --assess --type execute --verbose=4 /Applications/Tau.app
```

The assessment must report `accepted` and `source=Notarized Developer ID`. Do not use Telegram to test distribution: it can attach App Sandbox quarantine metadata that makes an otherwise valid app fail with `File created by an AppSandbox, exec/open not allowed`.
