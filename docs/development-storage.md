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
project directories, preferences, and nonsecret cloud connection configuration;
a sibling `sessions/` holds the Pi transcripts and per-project Tau session registries.
Cloud credentials are memory-only in development; no production Keychain entries
are read or changed. Reports and telemetry are never stored in the profile. Even importing
the same local project into two dev apps gives each its own session directory.
The previous persistent `tau-dev` profile and existing Pi transcripts are neither
loaded, migrated, nor deleted.

The webview uses non-persistent storage, and window geometry is not restored or
saved. Frontend HMR keeps the native workspace alive. A native process restart,
including a Rust rebuild, creates a fresh seed and clears cloud connections. Clean
app exit stops Pi writers and removes the temporary directory. Telemetry delivery
is best-effort; pending in-memory uploads need not complete before exit. A crash or forced kill
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

## Opt-in native performance fixture

Set `TAU_DEV_FIXTURE=performance` when launching **native development** to replace
Atlas/Notes for that process only. This is an invented, deterministic filesystem
fixture inside the same disposable profile: seven local projects, one **simulated
remote metadata** project (`tau-perf.invalid`, not a reachable SSH host), 589
local v3 JSONL files totaling exactly 306.5 MiB, 434 registered local sessions
(423 archived, 155 unregistered files), and 2,389 remote metadata sessions
(2,388 archived). The first 120 local files are 4 KiB; the others contain
larger non-context padding entries. Every fifth file includes an abandoned
branch, every eleventh a compaction. These proportions approximate a storage
workload; padding and invented text are not real transcript history. The
fixture never reads production Pi transcripts. Plain `bun tauri dev` continues
to seed Atlas/Notes. Production builds cannot invoke this seed.

Use the real dev window and admin diagnostics to inspect input-to-paint,
readable content, runtime readiness, and persistence. Record sample counts
and environment separately; filesystem timings alone do not establish
user-visible latency. For cloud diagnostics, enable admin mode and connect Grafana
before measuring; use a development stack/destination to avoid mixing fixture
runs with production reports. See `docs/observability.md`.

## Browser development

Plain browser development installs a small mock IPC adapter with in-memory
workspace, transcripts, and sidebar width. Separate tabs do not share mutations.
Reloading resets them, including project display-name changes; existing browser
localStorage is untouched. Rename regressions may install a test-local persisted
name store to verify that a fresh workspace consumes confirmed labels; that fake
does not change the ordinary sandbox's reset behavior or prove disk persistence. Mock replies
are generated locally, with no model calls, credentials, real filesystem writes,
or SSH connections. This is a playground, not a substitute for native integration
checks or deterministic reproduction scenarios. The issue-report fixture and
browser scenario adapter also mock Linear/Grafana in memory; no credentials or
reports are sent to either service.

## Project display names

Project rename persists only an optional display name in the existing project
registry, for local and SSH projects alike. Records without a saved name keep
their location-derived default. Reimporting an existing identity preserves its
custom name; renaming does not move folders, rewrite Pi transcripts, change
session storage, or reconnect SSH.

Native development keeps confirmed names in its current disposable profile,
including across frontend reloads/HMR and fresh registry reads. Restarting the
native process intentionally creates a fresh profile and resets those names.
Production registry names survive application restarts. Native storage tests
verify persistence by reading the same saved registry again, independently of
the browser fake and development-profile reset policy.

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
