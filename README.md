# Tau

A Tauri desktop UI for the [Pi coding agent](https://pi.dev), built with Vue and Rust.

Tau keeps the original app's project and session layout while replacing the Native SDK frontend. Projects are stored in `~/Library/Application Support/tau/projects.json`; Tau-created Pi sessions remain registered in each Pi project session directory's `.tau.json` file.

## Development

Requirements:

- Bun
- Rust
- Pi available on `PATH`, or `TAU_PI_PATH` set to its executable

```sh
bun install
bun tauri dev
```

Tau exposes both Pi integration routes from the selector in the session header:

- **RPC · Rust process** (default) starts `pi --mode rpc` directly from Rust.
- **SDK · Node sidecar** starts a bundled sidecar script with Node and imports the SDK from the installed Pi package.

Both adapters use the same JSONL contract, Vue state, and project/session persistence. The SDK option is available when Pi was installed as a Node package and Node is discoverable on `PATH` or through `TAU_NODE_PATH`.

## Checks

```sh
bun run format
bun run test
bun run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
bun tauri build
```
