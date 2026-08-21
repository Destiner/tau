# Native backend

Rust/Tauri backend for OS integration, persistence, Pi processes, SSH, and telemetry.

## Commands

- `cargo fmt --manifest-path src-tauri/Cargo.toml` - Format Rust.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` - Check formatting.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` - Lint all targets.
- `cargo test --manifest-path src-tauri/Cargo.toml` - Run native tests.
- `cargo test --manifest-path src-tauri/Cargo.toml <name>` - Run a matching test.

Run these commands from the repository root.

## Structure

- `src/lib.rs` - App lifecycle, native menu/window behavior, managed state, and command registration.
- `src/pi.rs` - Local/remote Pi process lifecycle and JSONL RPC transport.
- `src/ssh.rs` - OpenSSH connection parsing, probing, and remote commands.
- `src/storage.rs` / `src/settings.rs` - Workspace/session persistence and Pi settings.
- `src/telemetry/` - Native OpenTelemetry ingestion, local segments, retention, privacy, and optional export.
- `src/profile.rs` - Isolated development and production profile paths.

## Patterns

- Expose user-reachable failures as `Result` values with actionable messages; do not panic across a Tauri command path.
- Keep Pi processes keyed by runtime ID and generation. Validate IDs/paths, bound stderr capture, and ignore output from replaced generations.
- Pi RPC is newline-delimited JSON over stdio. Preserve request IDs and emit lifecycle events without exposing raw child-process data to the UI.
- Remote projects use the system OpenSSH client and must remain non-interactive; preserve SSH config aliases/options and login-shell `PATH` resolution.
- Register new Tauri commands in `src/lib.rs` and update the corresponding frontend contract and tests.
- Telemetry persistence/export must remain bounded and best-effort; apply privacy sanitization before records leave their source layer.
- Keep focused Rust tests in each module's `#[cfg(test)]` section.
