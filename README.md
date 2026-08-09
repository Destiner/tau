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

The default integration starts `pi --mode rpc` directly from Rust. Rust owns the child process and JSONL transport; Vue handles Pi protocol state and rendering.

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
