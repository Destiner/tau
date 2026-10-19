# Tau

Native GPUI desktop UI for the Pi coding agent. The application is in `gpui/` (Cargo package `tau-gpui`, binary `tau`); there is no Vue/Tauri app or Playwright suite.

## Commands

- `bun install --frozen-lockfile` - Install repository tooling.
- `bun run prepare` - Prepare pinned, patched native dependencies.
- `bun run dev -- --isolated /absolute/profile [--project /absolute/project]` - Run development with mandatory explicit isolation.
- `bun run build` - Build the optimized native application.
- `bun run check` - Check tooling lint/typecheck and Rust formatting/Clippy.
- `bun run test` - Run deterministic Bun, shared Rust, GPUI library/editor and Metal tests; excludes native OS and real Pi acceptance.
- `bun run package` - Build a fresh ad-hoc macOS preview, not a public release.
- `bun run test:pi-contract` - Explicit installed-Pi compatibility canary, not a default dependency.
- After prepare, direct Cargo commands work from root with `--manifest-path gpui/Cargo.toml`.

## Structure

- `gpui/` - Canonical app, retained directory name to preserve workspace/cache paths; native source, tests and tooling.
- `crates/tau-core/` - Framework-independent storage, profiles, Pi processes and SSH services.
- `crates/tau-session/` - Pi RPC orchestration, runtime/session identity and recovery.
- `crates/tau-observability/` - Shared preferences, feedback and bounded telemetry.
- `scripts/` - Repository tooling and explicit Pi compatibility checks.

## Project rules

- Treat `docs/quality.md` as the product-quality contract. Fix rubric violations with regression tests.
- Pi owns extensions and session files. Preserve compatibility instead of creating another runtime or session format.
- Each live session owns an isolated Pi runtime. Stale responses, switching, failures and eviction must not leak state or lose drafts/workflow ownership. Never replay uncertain work.
- Development requires explicit isolation. A normal product launch uses shared production storage; never run old and native apps simultaneously against it. Isolation is not a tool/filesystem sandbox; Vue drafts are not guaranteed to migrate.
- Telemetry is bounded, content-free and best-effort. Never record prompts, transcripts, drafts, payloads, paths, credentials, stderr or raw errors.
- Telemetry and the reporter are off by default and unlocked together by `iddqd` outside text fields. Keep diagnostics behind that gate.
- Prefer deterministic fakes. Native OS/installed-Pi/model/SSH acceptance is explicit, not part of default checks.
- Public native distribution and the updater are not implemented. Use ad-hoc previews only; preserve preview identity `com.destiner.tau.native-preview` and version `0.2.1` unless separately authorized.

## Documentation

- `gpui/APP.md` - Application behavior, lifecycle and accepted/deferred verification boundaries.
- `gpui/PACKAGING.md` and `docs/releases.md` - Preview packaging and blocked public distribution; release requests use `.agents/skills/release/SKILL.md`.
- `docs/development-storage.md` - Persistent isolated profiles and production storage safety.
- `docs/ci-testing.md` and `docs/reproduction.md` - Native test gates, Metal requirements and regression authoring.
- `docs/pi.md`, `docs/observability.md`, `docs/keyboard.md` - Runtime, privacy and interaction contracts.
