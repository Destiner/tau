# Tau native application

This directory contains the canonical GPUI app, not a prototype. Cargo package
`tau-gpui` builds binary `tau` from `src/bin/tau.rs`. The directory name remains
`gpui/` to avoid workspace and cache migration.

Use the repository-root commands in [README.md](../README.md): frozen Bun install,
`bun run prepare`, then `bun run dev -- --isolated /absolute/profile` or
`bun run build`. After preparation, direct Cargo commands work from root:

```sh
cargo build --locked --manifest-path gpui/Cargo.toml --bin tau --release
cargo test --locked --manifest-path gpui/Cargo.toml --lib
```

- [APP.md](APP.md) - Product behavior, lifecycle and verification boundaries.
- [PACKAGING.md](PACKAGING.md) - Fresh ad-hoc preview bundles.
- [Release safety](../docs/releases.md) - Production candidate infrastructure (in review), protected signing flow and public-release blockers.
- [CI and tests](../docs/ci-testing.md) - Deterministic suites, Metal and explicit OS acceptance.
- [Development storage](../docs/development-storage.md) - Isolation and production safety.
- [Dependency patches](patches/README.md) - Pinned native dependency maintenance.
- [Native host](src/native/README.md) - Platform adapters and retained editor support.

`tau-core`, `tau-session` and `tau-observability` own shared services; Pi owns
extensions and session files. Do not add another runtime or transcript format.

Metal tests need a real Metal device and full Xcode/Metal tools; there is no fake
renderer fallback. If `xcrun metal --version` reports a missing toolchain, install
Apple's component with `xcodebuild -downloadComponent MetalToolchain`.

## Assets and provenance

The bundled Latin Inter font was losslessly decompressed from the project's
`@fontsource-variable/inter` WOFF2 asset. Its native family name is `Inter`;
its license is retained in `assets/Inter-LICENSE.txt`. macOS monospace uses
`.AppleSystemUIFontMonospaced`; fallback glyphs and rasterization remain
platform-dependent. Preserve asset source notices, dependency licenses and diagram
license inventories when changing bundled assets. Removing comparison tooling is
not permission to remove legal provenance.
