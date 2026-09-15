# Development storage

## Default workspace

Native `bun tauri dev` and browser `bun run dev` start with the same seed:

- `atlas`: Workspace overview, Navigation review, Release checklist
- `notes`: Triage inbox, Markdown export

The single fixture is `src/dev/workspace-seed.json`. It contains only invented
content. There is one default template, no template selector or persistence flag.
Explicit `bun run repro -- <scenario>` and `?test-scenario=` retain their existing
scenario-specific behavior.

## Native development

Each native process creates a private `tau-dev-*` temporary directory. The app
prints its Tau data directory at startup. It holds workspace metadata, sample
project directories, preferences, feedback, and telemetry; a sibling `sessions/`
holds the Pi transcripts and per-project Tau session registries. Even importing
the same local project into two dev apps gives each its own session directory.
The previous persistent `tau-dev` profile and existing Pi transcripts are neither
loaded, migrated, nor deleted.

The webview uses non-persistent storage, and window geometry is not restored or
saved. Frontend HMR keeps the native workspace alive. A native process restart,
including a Rust rebuild, creates a fresh seed. Clean app exit stops Pi writers,
flushes telemetry, and removes the temporary directory. A crash or forced kill
can leave temporary files behind, but no subsequent launch reuses them; they can
be removed using the OS's normal temporary-file cleanup.

Pi is real: Tau preserves `PI_CODING_AGENT_DIR` and existing auth, models,
settings, and extensions. `--session-dir` redirects local sessions into the
current profile, including ordinary new-session/fork workflows. Seed transcripts
are Pi v3 JSONL and load through the normal workspace and RPC paths. They do not
pin a model; Pi chooses from the developer's configuration.

This is **state isolation, not a tool or filesystem sandbox**. Pi can modify real
files in projects you import. Pi configuration/auth, extension-owned storage, and
remote SSH sessions retain their normal behavior. An extension explicitly opening
a transcript outside the profile can leave the session directory too; Tau does
not replace Pi's session runtime or intercept extension execution. Use the sample
projects or browser mocks for disposable experimentation.

## Browser development

Plain browser development installs a small mock IPC adapter with in-memory
workspace, transcripts, and sidebar width. Separate tabs do not share mutations.
Reloading resets them; existing browser localStorage is untouched. Mock replies
are generated locally, with no model calls, credentials, real filesystem writes,
or SSH connections. This is a playground, not a substitute for native integration
checks or deterministic reproduction scenarios.

## Production and architecture

The native `dev` compile-time configuration selects ephemeral storage; production
has no environment override that can accidentally enable it. `bun tauri build`
keeps the existing application-data `tau/` directory, Pi session paths, webview
persistence, and window-state plugin. The browser adapter and seed are excluded
from the production frontend, and native seed code is excluded from production.

`src-tauri/src/profile.rs` owns storage locations and lifetime, independently of
workspace mutations. Both native profiles deliberately use the same file-backed
storage implementation: Pi owns session files, and exercising the real parser,
registry writes, and RPC resume path is more useful than a separate in-memory
native backend. Tests construct independent profiles without changing process
`HOME` or global Pi configuration.
