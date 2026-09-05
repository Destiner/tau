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
