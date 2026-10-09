# tau-session

Standalone, framework-independent Rust session controller for installed Pi's **actual JSONL RPC**. Dependencies: `std`, `serde`, `serde_json`. No process launcher, backend, GPUI, Tauri, model calls, credentials, session-file writer, or extension runtime.

## Host integration

```rust
use serde_json::Value;
use tau_session::session::{Controller, Effect};

let mut controller = Controller::new("runtime-1");
// For reopening only: bind the registry's requested identity before starting Pi.
controller.bind_source("saved-id", "/project/session.jsonl");
// Host starts Pi --mode rpc [--session <path>] and obtains a new generation.
let bootstrap = controller.begin_generation(1);
// Dispatch each Send JSON value as one complete JSON object followed by LF.
// Capture runtime_id + generation alongside every asynchronous dispatch.
let effects = controller.ingest("runtime-1", 1, Value::Null);
let _ = (bootstrap, effects);
```

Use ordinary persistent RPC mode for saved work; **`--no-session` means in-memory** and cannot produce a registration effect without a path. Host owns trust/working directory/extension discovery options. Pi owns session formats, credentials, extensions and execution.

### API

`tau_session::session::Controller`:

- `new(runtime_id)`, `runtime_id()`, `generation()`, `state()`, `pending_count()`.
- `bind_source(id, path)` **before bootstrap**, not a live session-switch API.
- `begin_generation(generation) -> Vec<Effect>` accepts strictly increasing generations, recovers unconfirmed prompts and bootstraps `get_state`, `get_messages`, `get_available_models`, `get_commands`. Old/duplicate starts are no-ops.
- **`ingest(runtime_id, generation, Value)`** is the safe entry for process stdout. `handle(Value)` is only for records already fenced by the caller. Both return effects; inspect `state()` afterward for changes.
- `set_draft(text)`, `submit(text)`, `submit_queued(text, QueueKind::{Steer, FollowUp})`.
- `abort()`: Stop current work, never clear queues. `queue_snapshot()`, `clear_queue()`.
- `set_model(provider, model_id)`, `set_thinking(level)`, `refresh()`.
- `extension_response(id, ExtensionResponse::{Value(String), Confirmed(bool), Cancelled})`. Dialog remains `submitting` until the host calls **`extension_sent(id, success)`** after writing the response. Pi does not emit a normal response to this record. On write failure the dialog is retryable. Fence host callbacks by captured runtime/generation, just like stdout.
- `request_failed(request_id)` for failed transport writes of ordinary requests. `transport_failed(generation)` for process loss; preserves partial transcript and recoverable drafts, fences further output until a newer generation.
- `tick(now_ms)` using a **nondecreasing logical/monotonic clock** supplied by the host; defaults to zero before the first tick. Feed a tick before dispatching startup/actions, then periodically while connected. Startup, requests, accepted-but-not-started prompts, and Stop are bounded at 10 seconds. This is not a maximum duration for an active model/tool run.
- `take_recovered_drafts()` drains uncertain/lost queue text for a host-owned review surface. Never automatically resend it.

All action/effect APIs return `Vec<Effect>`; empty effects mean no request was dispatched. Keep text in the composer unless `submit` actually dispatched it. Submission rejection restores exact submitted text ahead of newer input. A timeout means **uncertain acceptance**, so its text goes to `recovered_drafts`, not automatic replay.

`State` exposes identity (`id`, `path`, `name`), opaque Pi `model`, `effort`, `status`, readiness/working/stopping/history-loading flags, `transcript`, draft/recovered drafts, queue snapshots, `extension_dialogs`, opaque commands/models, thinking levels, reviewed `error`, and `durable`.

`Entry` exposes stable UI `id`, `kind`, `text`, `activity`, `pending`, `interrupted`, tool ID/name/arguments/result/error and a source reconciliation key. These IDs are **controller-local UI IDs**, not Pi session-entry IDs or durable history cursors. Actual Pi message timestamps plus occurrence order bind streamed rows to hydration; tool rows use actual `toolCallId`. Final `message_end` text replaces deltas without changing row identity. Render/sanitize content in the host; this crate has no DOM or markdown renderer.

### Effects and durability

- `Send(Value)`: route to this controller's captured runtime/generation; the host must fence queued async writes too. Strict JSONL framing belongs to the transport.
- `RegisterSession { runtime_id, generation, id, path, name }`: register this **captured** identity in host storage, never select a UI row. Treat the effect as a storage instruction, not evidence that the write succeeded; retry storage failures in the host using the captured effect.
- `IdentityChanged { previous, current }`: runtime rebinding only; **no forced UI selection**. The host decides how to present/adopt/retire registry rows. Same requested path under a different ID yields session-local unsaved-source feedback, not silent substitution of the old registry identity.

A finalized assistant `message_end` dispatches an identity-scoped `get_state`. Its matching successful response is the ordering barrier after Pi's synchronous append/flush. Partial output, user-only history, notifications, command-only runs, and identities with no file path never register. A barrier that first reveals a successor registers the completed **predecessor** with its captured ID/path/name, then rebinds the successor without granting it durability. A newer identity epoch invalidates outstanding predecessor responses. IDs namespace runtime + generation + increasing request sequence; duplicate, unknown, mismatched, superseded, stale-identity and stale-generation responses cannot hydrate current state.

Queue messages use real `prompt` with `streamingBehavior: "steer" | "followUp"`, after serialized acknowledgements of `set_steering_mode: all` and `set_follow_up_mode: one-at-a-time`. **Pi persists these account defaults**, affecting other processes; they are not restored. Snapshots come only from `queue_update`. Clear acknowledges actual `clear_queue` without erasing a newer snapshot. Stop does not clear or replay queues.

## Exact first-slice limitations

This is a working protocol core, **not TS runtime parity**:

- History is the current Pi `get_messages` projection. No `get_entries` tree traversal, branch/fork/navigation, pre-compaction reveal, progressive history loading, or stable durable Pi entry cursors. Compaction success requests a fresh message projection; it is not the full TS compaction reconciliation algorithm.
- Timestamp/occurrence reconciliation assumes actual Pi message timestamps and append-order current history. Synthetic/malformed messages without timestamps, context rewrites, abandoned branches and compaction can change row IDs. Empty/image-only user messages have no image UI; skill envelopes are plain user text, not a dedicated skill row.
- Tool execution events and final assistant tool calls are projected. `toolcall_*` argument streaming is not displayed before the final tool-call/message or tool execution start. Provider-level `done`/`error` updates rely on ordinary Pi `message_end`. Tool/custom renderers and image results are not implemented. Nested tool execution rows may disappear on authoritative hydration because Pi does not store nested calls as independent messages.
- Replacement detection is limited to bootstrap, agent-start, settlement, handled-command, answered-dialog, explicit refresh, and durability reads. No delayed replacement watcher, retry/backoff, bounded materialization verification, saved-history registration fallback, registry archive mutation, idle eviction or workflow restoration. If a replacement is learned through another read before a predecessor barrier returns, that barrier is conservatively discarded; predecessor registration recovery belongs to a later slice. Events emitted for an undiscovered successor can transiently project under the predecessor and are discarded on rebinding; there is no event-carried identity to infer otherwise. No transfer of live successor stream suffix across that race.
- A loaded existing history does not itself emit registration; the registry already owns the requested saved source. Registration only follows a live assistant completion barrier. No rename RPC/registration update flow. `durable` means protocol proof, not successful host registry persistence.
- Extension select/confirm/input/editor/notify/set_editor_text are handled. TUI statuses/widgets/titles/custom components, extension shortcut/flag/autocomplete controls are intentionally ignored. Dialog requests without Pi timeouts are user waits, not forced 10-second expiries. No host dialog-answer draft persistence API; the shell owns answer controls/drafts. Transport failure drops dialogs; reconnect does not replay responses.
- No reconnect, process spawning/killing, JSONL byte framing, SSH, ownership fencing, storage, telemetry, retry policies, or UI-selection behavior. Use a new generation after reconnect; never replay a prompt automatically. Late callbacks must be fenced by the host. Active provider/tool hangs are not detectable without host health checks. Stop timeout is feedback, not proof Pi stopped.
- Errors are generic reviewed local copy; no provider-specific error classifier or retry activity detail. Local notices are retained on hydration but are not TS's fully anchored notice/compaction model. Transcripts/model catalogs are in-memory and not size-limited.

## Verification

Default contract tests follow installed Pi `docs/rpc.md`, `rpc-commands.md`, `json.md`, `rpc-extension-ui.md`, `message-types.md`, `session-format.md`, and Tau's runtime contracts. They use fake JSON and do not launch Pi, read session files, or invoke models. The separately ignored installed-Pi canary is explicit opt-in.

```sh
cargo fmt --check --manifest-path crates/tau-session/Cargo.toml
CARGO_BUILD_JOBS=2 cargo test --manifest-path crates/tau-session/Cargo.toml
CARGO_BUILD_JOBS=2 cargo clippy --manifest-path crates/tau-session/Cargo.toml --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --manifest-path crates/tau-session/Cargo.toml
```

The credential-free installed-Pi canary uses a temporary project/config/session tree
and an explicit trusted extension. It proves RPC and extension interaction, not model
streaming or assistant persistence:

```sh
TAU_LIVE_RPC_CHECK=1 cargo run --manifest-path crates/tau-session/Cargo.toml --example live_rpc_check
```

`TAU_LIVE_PI` optionally selects the installed Pi executable. No credentials or model
calls are needed. The native application has its own opt-in window acceptance target.
