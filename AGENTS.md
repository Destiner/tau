# Tau

Native Tauri desktop UI for the Pi coding agent, with a Vue frontend and Rust backend.

## Commands

- `bun install` - Install frontend tooling.
- `bun tauri dev` - Run the desktop app; requires Pi on `PATH` or `TAU_PI_PATH`.
- `bun run build` - Typecheck and build the frontend.
- `bun run format` / `bun run lint` - Format or run all frontend linters.
- `bun run typecheck` - Check Vue and Node TypeScript.
- `bun run test` - Run Vitest unit tests.
- `bun run test:e2e` - Run Playwright in Chromium and WebKit.
- `bun run repro -- --list` - List deterministic Pi scenarios.
- `bun run repro -- <scenario>` - Open a scenario in the development app.
- `bun run test:pi-contract` - Explicitly check the installed Pi RPC contract.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` - Check Rust formatting.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` - Lint Rust.
- `cargo test --manifest-path src-tauri/Cargo.toml` - Run Rust tests.
- `bun tauri build` - Build the distributable app.

Install Playwright once with `bun x playwright install chromium webkit`.

## Structure

- `src/` - Vue UI, session state, Pi RPC client, and frontend telemetry.
- `src-tauri/` - Tauri commands, Pi/SSH process management, storage, and native telemetry.
- `tests/e2e/` - Browser-visible behavior and full-app scenario tests.
- `tests/support/pi-scenario/` - Typed deterministic Pi protocol scenarios.
- `scripts/` - Interactive reproduction and Pi compatibility utilities.

## Project rules

- Treat `docs/quality.md` as the product-quality contract. Fix rubric violations with regression tests.
- Pi owns extension execution and session files. Preserve Pi compatibility rather than creating a parallel runtime or data model.
- Each live session owns an isolated Pi runtime. Session switching, stale responses, failures, and idle eviction must not leak state or lose drafts/workflow context.
- Telemetry is bounded, content-free, and best-effort. Never record prompts, transcripts, drafts, payloads, paths, credentials, stderr, or raw errors.
- Prefer deterministic fake-based tests; the real-Pi contract check is an explicit canary, not a default test dependency.

## Documentation

- `docs/extensions.md` - Extension support matrix, runtime lifetime, and session replacement behavior.
- `docs/observability.md` - Telemetry storage, privacy limits, issue reports, and optional OTLP export.
- `docs/quality.md` - Product principles and the verification rubric for all UI work.
- `docs/reproduction-scenarios.md` - Scenario authoring, interactive controls, regression workflow, and Pi canary scope.
