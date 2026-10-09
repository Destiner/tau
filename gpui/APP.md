# Native Tau application

`src/bin/tau.rs` is the application entrypoint. It uses real workspace storage and Pi
processes through `tau-core`, with RPC orchestration in `tau-session`. It does not
import the synthetic conversation or comparison scene runtime.

This is the first operational native slice, not the Tauri replacement release.
The current delivery gates and remaining work are in [the rewrite plan](../docs/gpui-rewrite.md).

## Run

Prepare the pinned native dependencies once, then build:

```sh
bun run --cwd gpui prepare:native
CARGO_BUILD_JOBS=2 cargo build --manifest-path gpui/Cargo.toml --bin tau
```

Use an explicitly isolated directory during development and validation:

```sh
cargo run --manifest-path gpui/Cargo.toml --bin tau -- --isolated /absolute/path/to/native-profile --project /absolute/path/to/project
```

`tau` is also Cargo's `default-run` target; the older `src/main.rs` playground is
retained separately. The directory is persistent across launches. Tau metadata and native UI preferences
live under `tau/`; local Pi sessions live under `pi-sessions/`. There is no automatic
synthetic seed. `--session <registered-id-or-path>` selects an existing registered
session. **Isolation is not a tool or filesystem sandbox**: ordinary Pi retains its
credentials, extensions and ability to act in the selected project.

Without `--isolated`, an explicit user launch loads the existing production Tau profile.
Do not run both shells against that profile during migration: cross-process profile
locking and production cutover have not been certified. There is no destructive data
conversion. Pi owns transcripts; native UI preferences/drafts use `native-ui.json`.

## Implemented boundary

- Real local workspace import, session selection and new sessions; existing remote
  project configurations are recognized and can use the shared SSH transport.
  Real SSH smoke acceptance and new remote import UI remain outstanding.
- Retained composer/transcript entities, full-height empty composer, model/effort
  choices, visibly rendered prompts/streaming prose/activity, Stop, queue submission
  and clear.
- Click-to-rename saved sessions through Pi, with bounded names, rollback and registry
  persistence. Archive idle saved sessions; browsing archives does not restore them,
  and Restore is explicit. Drafts and session files are retained.
- Extension input/select/confirm, notifications and editor-text requests. The extension
  multiline editor still needs its dedicated control.
- Application menus, theme settings, owned folder chooser, guarded close/quit,
  draft persistence, warm runtimes while the last window is closed, and reopen.
- Bounded presentation cache and incremental changed-row projection. Streaming does
  not hash or clone unchanged historical text.

The current slice does **not** finish session removal, palette, remote
import UI, file previews/media, admin reporter, full history/replacement recovery,
idle eviction, updater or release packaging. Unavailable actions report that boundary;
they are not simulated successes. See [controller limits](../crates/tau-session/README.md).

## Verification

Default Rust tests use deterministic fakes. Installed Pi and an unlocked macOS desktop
are needed only for explicitly selected acceptance targets:

```sh
CARGO_BUILD_JOBS=2 cargo test --manifest-path gpui/Cargo.toml --lib
CARGO_BUILD_JOBS=2 cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path crates/tau-core/Cargo.toml
cargo test --manifest-path crates/tau-session/Cargo.toml

# Own native window + installed Pi, isolated and credential-free, no model call:
CARGO_BUILD_JOBS=2 cargo test --manifest-path gpui/Cargo.toml --features native-os-tests --test native-app

# Explicit model calls using existing Pi auth and only an invented cancellable tool:
TAU_NATIVE_MODEL_SMOKE=1 CARGO_BUILD_JOBS=2 cargo test --manifest-path gpui/Cargo.toml --features native-os-tests --test native-app
```

The native acceptance target drives renderer-measured controls, native menus and editor
handlers in its own process. It does not use clipboard access, cross-app automation,
permission prompts or fabricated Pi events. Command-only custom output must remain
ephemeral; it is not proof of assistant streaming or transcript durability. The model
variant separately requires actual assistant text deltas, assistant end, idle state,
a durable session file and successful registry persistence. It then asks the model
to invoke one fixture-only tool with no filesystem, network or process access,
clicks the measured native Stop button, and requires real abort/tool-end/settlement
with partial output and unrelated draft preservation.

Credential-free native acceptance with Pi 0.99.2 passed, including actual extension
input/Enter, custom output, drafts/session switching, Settings, close/reopen,
busy-close cancellation and quit cleanup. Reopen has a measured-control regression
with a negative control for stale first-paint listeners (12 application tests passed).
This is not assistant acceptance or a repeated native reliability campaign.

The first optional model run failed its unchanged 60-second gate. Its recorder
incorrectly expected `message_update.message.role`; actual Pi updates are delta-only.
The corrected recorder uses `assistantMessageEvent.text_delta`. Corrected model
validation **passed**: stream/end/text/idle/durability/file/registry all observed,
followed by reopen/switch, busy-close cancellation and gated quit cleanup. Earlier
failures are retained, with no widened deadline. This proves the small local
assistant slice. The subsequent real-tool/Stop slice also passed, including actual
Pi start/partial/end events and abort acknowledgement through the rendered Stop
button. This does not prove interruption during provider token streaming, populated
queue preservation, SSH or full runtime parity. Hovered sidebar membership now
reconciles arriving/removing sessions without losing its held order, with an
old-code negative control. The native library (263 tests), editor integration tests
(13), strict all-target/all-feature Clippy,
and actual `tau` build passed. The full Rust run timed out in the existing Metal
activity runner, so it is not reported as an all-suite pass. Historical curated
counts and the unsigned Tauri bundle are not GPUI release proof. No signed/notarized
GPUI distribution or updater is available yet. A separate, locally ad-hoc-signed
`Tau Native.app` preview can be built using [PACKAGING.md](PACKAGING.md); it neither
replaces Tauri nor isolates data unless explicitly launched with `--isolated`.

The reported layout/title/action regressions have production-root layout and paint
coverage, plus inspected native Metal images. Real Pi acceptance passed rename and
registry persistence, Escape cancellation, archive/browse/restore, and composer
geometry. That extended model run later timed out at Cmd-W after the window lost
native activation; it is not a complete smoke PASS. The final credential-free
native run passed, including close/reopen and gated shutdown.
