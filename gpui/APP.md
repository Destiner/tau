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

- Local and remote project import, session selection and new sessions. Remote import
  uses the existing SSH probe/directory browser and storage contracts; network reads
  do not block Pi writes or Stop. Authenticated real-host SSH acceptance remains outstanding.
- Retained composer/transcript entities, full-height empty composer, model/effort
  choices, visibly rendered prompts/streaming prose/activity, Stop, queue submission
  and clear. While working, Enter or Send queues steering; Cmd-Enter queues a follow-up.
  Shift-Enter inserts a newline. When idle, Cmd-Enter sends an ordinary prompt.
- Cmd-K command palette with real session/project/model/effort pages, supported actions
  and shortcuts, search, keyboard navigation, and modal-aware focus restoration.
- Model choices use Pi's scoped-model settings for local or remote sessions, following
  Vue's matching/order/fallback behavior. An out-of-scope current model remains a
  display label, not an extra selectable option. Provider-grouped, searchable menus
  scroll within the viewport; thinking labels are human-readable (including Extra High).
  Choice revisions avoid rebuilding or serializing the catalogue during streaming.
- Thinking/tool adjacency uses Vue's compact spacing; tool headers include argument
  summaries such as `read  gpui/APP.md`, with unchanged raw source and results.
  Expanded details own wheel scrolling, including at either edge, and retain their
  offset across updates/cache eviction. The transient Working/Stopping tail follows
  Vue's assistant/compaction rules and is excluded from transcript data and Copy.
- Click-to-rename saved sessions through Pi, with bounded names, rollback and registry
  persistence. Archive idle saved sessions; browsing archives does not restore them,
  and Restore is explicit. Drafts and session files are retained. Archives are
  virtualized, globally sorted within local-calendar groups and include project/model
  subtitles. Sidebar projections are invalidated at data/status boundaries rather than
  rebuilt for every text chunk; relative times and groups also age while idle.
- Draft/unread indicators, rich session status/message/time tooltips, SSH project
  alias/directory tooltips, and animated edge-reveal drag handles.
- `iddqd` outside text fields unlocks the persisted shared admin/telemetry gate and
  retained issue reporter. Reports use the existing local feedback service, with an
  unchecked session attachment option. Palette reporting is available only in admin mode.
  Accepted report writes settle even when hidden; no timer invents a retryable failure.
- Extension input/select/confirm, notifications and editor-text requests. The extension
  multiline editor still needs its dedicated control.
- Application menus without a Settings surface or Cmd-comma shortcut, owned folder
  chooser, guarded close/quit and draft persistence. Closing the last production
  window quits only after confirmation (when busy), all-draft persistence and
  runtime shutdown acknowledgement. Closing one of multiple windows keeps the app alive.
- Bounded presentation cache and incremental changed-row projection. Streaming does
  not hash or clone unchanged historical text.

The current slice does **not** finish session removal, unsupported palette commands
(such as removal, queue recovery or update actions), file previews/media,
full history/replacement recovery,
idle eviction, updater or release packaging. Unavailable actions report that boundary;
they are not simulated successes. See [controller limits](../crates/tau-session/README.md).

## Keyboard and remote projects

Cmd-K opens the palette; Cmd-P / Cmd-Shift-P open sessions/projects, and
Cmd-Shift-M / Cmd-Shift-E open model/thinking choices. Cmd-Shift-O opens a local
project; Cmd-Alt-O opens the SSH form. Cmd-period stops active Pi work.

In the SSH form, Enter probes the connection. The directory browser supports filtering,
Up/Down, Enter to choose the highlighted row, Cmd-Up for the parent, and Cmd-Enter to
import the current directory. Escape/backdrop cancels when idle. Reopening starts at
the connection step while retaining its draft. Errors are local and retryable; busy
imports cannot be repeated or discarded by approving Quit. Storage imports are serialized
with other workspace writes and merge the returned project instead of a stale snapshot.

## Last-window close policy

Production opts into `host.lifecycle().set_quit_on_last_window_close(true)` before
opening its first window. The generic NativeHost retains explicit quit and last-window
reopen for independent host tests. GPUI's platform quit mode stays Explicit: automatic
last-window quitting would remove the root before persistence/shutdown could veto it.
Red close, Cmd-W and File → Close Window all enter the same guard; last close becomes
`Intent::Quit` before the production callback runs. Cmd-Q keeps its existing guard.

If concurrent Close requests leave one owner, approval is promoted to a fresh guarded
Quit request, superseding pending Close ownership and retaining the original monotonic
deadline. Dispatch rechecks too, covering two approvals queued in one event turn.
A Close approval never authorizes Quit. Cancellation, failed persistence and expiration
leave the remaining root and editor intact; late approvals cannot remove it.

Native-app acceptance no longer closes/reopens the last production window. It preserves
the palette/remote, assistant/tool, rename/archive and session-switch sequence, cancels
a busy last Cmd-W with root/editor/draft/focus assertions, completes the extension, then
finishes with last Cmd-W. The harness requires completion-gated app exit and every
installed-Pi child exit acknowledgement before harness process-group cleanup. Generic
host reopen coverage remains separate. Historical close/reopen results below predate
this production policy. Both credential-free and model-enabled native acceptance
passed the new guarded last-Cmd-W policy on 10 October 2026.

## Verification

The sidebar/admin batch passed **370 library tests** (one ignored timing benchmark),
**13 editor integration tests**, **48 controller tests** (one opt-in Pi canary ignored),
strict checks and the actual native build. Both native acceptance variants passed:
measured dirty-dot focus, raw admin unlock outside editors, refusal inside the reporter,
retained Escape/reopen, real optional-checkbox toggles, exactly one isolated shared
report without a session attachment, persisted admin disable, and guarded last-window
quit after all Pi child exit acknowledgements. The model variant also passed real
assistant/tool/Stop, rename and archive/restore. Owned report/transcript images were
inspected; this is not a fresh full Vue/native screenshot campaign or SSH acceptance.

Initial integrated archive failures exposed width/start-anchor problems and an
uninitialized test viewport. Native acceptance first exposed an invalid explicit-false
checkbox predicate (absence means enabled), then a real pointer-dismissal focus steal.
The corrected test exercises both checkbox states; pointer dismissal now preserves the
clicked control. Original failures remain recorded, without reactivation or gate changes.
A model-choice refactor also briefly shadowed the identity-change flag; the existing
predecessor-durability regression caught it and passed after the correction.

The older acceptance results below are checkpoint-specific, not current failures or
release certification.

Production-root keyboard regressions check follow-up versus steering in the actual RPC
request after queue-mode acknowledgments, including remounts, session isolation and idle
submission. Composer tests also protect the IME composition guard.

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
input/Enter, custom output, drafts/session switching, native menus, close/reopen,
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

The workflow batch adds production-root activity geometry, argument-summary, scoped
picker, palette and remote-flow regressions. Native Metal captures of the palette,
SSH form and tool transcript were inspected. Credential-free native acceptance passed
Cmd-K → search/Enter → remote form → Escape → palette reopen/dismiss, with retained
composer draft/focus, followed by close/reopen and shutdown. Two initial acceptance
runs exposed an invalid test predicate: InputBase snapshots do not contain editor
focus/value facts. The corrected gate checks measured bounds plus the actual InputState
focus and value; its deadline is unchanged. The model variant passed assistant/tools,
Stop and session actions, then again timed out at the later Cmd-W gate; it is not a
complete model smoke pass. Directory navigation/import uses deterministic fake-command
and isolated-storage tests, not an authenticated SSH connection. There was no fresh
full Vue/native screenshot comparison campaign.

The reported layout/title/action regressions have production-root layout and paint
coverage, plus inspected native Metal images. Real Pi acceptance passed rename and
registry persistence, Escape cancellation, archive/browse/restore, and composer
geometry. That extended model run later timed out at Cmd-W after the window lost
native activation; it is not a complete smoke PASS. The final credential-free
native run passed, including close/reopen and gated shutdown.
