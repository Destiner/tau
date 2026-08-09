# Tau

A Tauri desktop UI for the [Pi coding agent](https://pi.dev), built with Vue and Rust.

Tau keeps the original app's project and session layout while replacing the Native SDK frontend. Production projects are stored in `~/Library/Application Support/tau/projects.json`; development builds use an isolated `tau-dev` app profile and `.tau-dev.json` session metadata. Both profiles keep the underlying Pi session files in the same Pi project session directories.

## Development

Requirements:

- Bun
- Rust
- Pi available on `PATH`, or `TAU_PI_PATH` set to its executable

```sh
bun install
bun tauri dev
```

Tau includes both Pi integration routes:

- **RPC · Rust process** (default) starts `pi --mode rpc` directly from Rust.
- **SDK · Node sidecar** starts a bundled sidecar script with Node and imports the SDK from the installed Pi package.

Both adapters use the same JSONL contract, Vue state, and project/session persistence. RPC is selected by `activePiIntegration` in `src/lib/pi-integrations.ts`; change that value to `"sdk"` to run the Node sidecar instead. The SDK adapter is available when Pi was installed as a Node package and Node is discoverable on `PATH` or through `TAU_NODE_PATH`.

Each live session owns an isolated Pi runtime. Switching the visible session does not interrupt running work in other sessions; hidden idle runtimes are released and restored from their session files when selected again. Session lists are ordered by the latest user message, so background agent events do not move rows. Removing a project stops all of its runtimes, and quitting Tau stops every child process.

Remote projects use the system OpenSSH client and start directory browsing from the remote account's default working directory. SSH config aliases and command-line options are supported. Authentication must work non-interactively, such as through keys or an SSH agent, and Pi must be available on the remote login shell's `PATH`.

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
