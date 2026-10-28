# Tau

Native Tauri desktop UI for the Pi coding agent, with a Vue frontend and Rust backend.

## Commands

- `bun install` - Install frontend tooling.
- `bun tauri dev` - Run the desktop app; requires Pi on `PATH` or `TAU_PI_PATH`.
- `bun run build` - Typecheck and build the frontend.
- `bun run format` / `bun run lint` - Format or run all frontend linters.
- `bun run typecheck` - Check Vue and Node TypeScript.
- `bun run test` - Run Vitest unit tests.
- `bun run test:e2e` - Run Chromium functional Playwright tests (the default, including when `CI=1`).
- `bun run test:e2e:webkit` - Preflight WebKit, then run WebKit functional tests only.
- `bun run test:e2e:ci` - Preflight WebKit, then run both functional browser projects; CI uses `bun run test:e2e:ci --retries=1`.
- `bun run test:e2e:performance` - Run the two isolated Chromium performance guards (one worker, tracing off, zero retries).
- `bun run repro -- --list` - List deterministic Pi scenarios.
- `bun run repro -- <scenario>` - Open a scenario in the development app.
- `bun run test:pi-contract` - Explicitly check the installed Pi RPC contract.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` - Check Rust formatting.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` - Lint Rust.
- `cargo test --manifest-path src-tauri/Cargo.toml` - Run Rust tests.
- `bun tauri build` - Validate the full native build; on macOS, produces only an unsigned `.app` without opening Finder. Use this for routine agent validation, not DMG packaging.
- `bun run release:macos` - Build and verify the Apple Silicon, signed, notarized macOS DMG.

Install Playwright once with `bun x playwright install chromium webkit`. Browser runs always build and own a fresh preview server; an occupied default port fails instead of reusing another server. Set `TAU_PLAYWRIGHT_PORT` to an unused port when needed, and serialize browser runs in one worktree because they share `node_modules/.cache/tau-e2e`.

The standard agent macOS VM cannot currently start WebKit reliably. Use Chromium for routine local validation; reserve `test:e2e:webkit` for a working desktop or bounded diagnosis, and let CI execute the required WebKit coverage.

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
- Telemetry and the issue reporter are admin mode's, off by default, and unlocked together by the `iddqd` code. Keep new diagnostics behind the same switch.
- Prefer deterministic fake-based tests; the real-Pi contract check is an explicit canary, not a default test dependency.

## Documentation

- `docs/ci-testing.md` - CI gates, browser test execution, and reproducible timing.
- `docs/development-storage.md` - Isolated development profiles, browser sandbox, and performance fixtures.
- `docs/pi.md` - Pi extension compatibility, runtime lifetime, and session replacement behavior.
- `docs/observability.md` - Telemetry storage, privacy limits, issue reports, and optional OTLP export.
- `docs/quality.md` - Product principles and the verification rubric for all UI work.
- `docs/releases.md` - Signing infrastructure, updater trust, local verification, and release recovery; publication uses `.agents/skills/release/SKILL.md`.
- `docs/reproduction.md` - Scenarios, manual fixtures, regression authoring, and the explicit Pi canary.
