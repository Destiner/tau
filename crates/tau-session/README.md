# tau-session

Framework-independent Rust session controller for installed Pi's **actual JSONL RPC**. Uses `serde`, `serde_json` and the shared skill-envelope parser in `tau-core`. The controller itself does not launch processes, access credentials/session files, invoke models, or execute extensions, and has no GPUI/Tauri dependency.

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
- `begin_generation(generation) -> Vec<Effect>` accepts strictly increasing generations, recovers unconfirmed prompts and bootstraps `get_state`, `get_messages`, `get_available_models`, `get_commands`, and an optional `get_scoped_models` capability probe. Old/duplicate starts are no-ops.
- **`ingest(runtime_id, generation, Value)`** is the safe entry for process stdout. `handle(Value)` is only for records already fenced by the caller. Both return effects; inspect `state()` afterward for changes.
- `set_draft(text)`, `submit(text)`, `submit_queued(text, QueueKind::{Steer, FollowUp})`.
- `abort()`: Stop current work, never clear queues. `queue_snapshot()`, `clear_queue()`.
- `set_model(provider, model_id)`, `set_thinking(level)`, `refresh()`, `refresh_models()`.
- `model_choices_revision()`, `commands_revision()` let hosts cache catalogue projections across unchanged streaming updates. Command revisions advance only when the authoritative catalogue changes; generation/identity replacement clears it, and existing request/generation/identity fences reject stale replies.
- `load_history(marker_id)` reveals one earlier compaction layer, using cached active-branch entries or a fenced `get_entries` request. Reconcile the authoritative marker state even when no effects are returned: a cached reveal and a rejected request are both possible.
- `setting_pending()`, `submission_pending()`, `queue_busy()` expose action-specific admission instead of treating pending read requests as blocked work.
- `begin_model_scope_read() -> ModelScopeToken`, `apply_model_scope(&token, Option<Value>)`: fence host-owned async settings reads by generation, identity and refresh/read revision. The value is an array of scope patterns, not the entire settings file.
- `extension_response(id, ExtensionResponse::{Value(String), Confirmed(bool), Cancelled})`. Dialog remains `submitting` until the host calls **`extension_sent(id, success)`** after writing the response. Pi does not emit a normal response to this record. On write failure the dialog is retryable. Fence host callbacks by captured runtime/generation, just like stdout.
- `request_write_accepted(generation, request_id) -> bool` transfers a pending request to the host transport owner after successful write admission. Call before a tick can expire it, not only on a delayed physical-write receipt. This is not Pi's semantic acceptance: known extension `prompt` handlers may await preflight or UI indefinitely before their correlated reply. Only those admitted commands leave the 10-second request/agent-start deadline; their reply or transport loss still settles ownership. Generation, request, identity and epoch fence admission. Ordinary prompt/read/queue/Stop timeout policy is unchanged.
- `error_token() -> Option<ErrorToken>` captures the displayed occurrence; `acknowledge_error(&token) -> bool` dismisses only that runtime/generation/epoch/incident. Capture with the presented error, not at dismissal time. Dismissal never acknowledges an uncertain write, ends reconciliation, or releases a provisional replacement.
- `request_failed(request_id)` for correlated ordinary write failure; `input_closed(generation)` blocks new work but retains same-generation final output through `ingest_output(runtime_id, generation, Value)`. Input closure is not output termination. `transport_failed(generation)` fences output after authoritative process/output loss. `backend_stopped(generation)` reconciles an exact physical cleanup receipt even if idle-release admission changed, preserving drafts/transcript/proof and rejecting old output without disconnecting a newer generation.
- `prepare_exit()` hands every captured completed-output intent to `RegisterSession`, including predecessor/old-generation proof; `durability_pending()` reports outstanding protocol proof, not exit permission or storage success. Hosts must drain effects and await exact storage/recovery/cleanup receipts, including newly completed work during Save.
- `reconciliation_pending()`, `provisional_replacement()`, `source_recovery()` and exact `acknowledge_source_recovery(&Identity)` expose bounded replacement/recovery ownership. `can_release_runtime()` excludes work, queue/dialog/read/uncertainty/provisional/proof owners; call `release_generation(generation)` only after successful physical cleanup. `resume_reads()` retries reconciliation, never the user operation.
- `saved_preview_token()` / `apply_saved_preview(&token, messages)` admit a fenced cosmetic projection before authoritative RPC. Preview cannot establish readiness, runtime identity, registration or durability.
- `tick(now_ms)` using a **nondecreasing logical/monotonic clock** supplied by the host; defaults to zero before the first tick. Feed a tick before dispatching startup/actions, then periodically while connected. Startup, ordinary requests, accepted-but-not-started ordinary prompts, and Stop are bounded at 10 seconds. Admitted extension commands retain their correlated owner until reply or transport loss, including while no agent run has started. This is not a maximum duration for an active model/tool run.
- `take_recovered_drafts()` drains uncertain/lost queue text for a host-owned review surface. Never automatically resend it.

All action/effect APIs return `Vec<Effect>`; empty effects mean no request was dispatched. Keep text in the composer unless `submit` actually dispatched it. Submission rejection restores exact submitted text ahead of newer input. A timeout means **uncertain acceptance**, so its text goes to `recovered_drafts`, not automatic replay.

`State` exposes identity (`id`, `path`, `name`), opaque Pi `model`, `effort`, `status`, readiness/working/stopping/history-loading flags, `transcript`, draft/recovered drafts, queue snapshots, `extension_dialogs`, opaque commands/models, thinking levels, reviewed `error`, and `durable`.

`Entry` exposes stable UI `id`, `kind`, `text`, `activity`, `pending`, `interrupted`, tool ID/name/arguments/result/error and a source reconciliation key. These IDs are **controller-local UI IDs**, not Pi session-entry IDs or durable history cursors. Actual Pi message timestamps plus occurrence order bind streamed rows to hydration; tool rows use actual `toolCallId`. Final `message_end` text replaces deltas without changing row identity. Render/sanitize content in the host; this crate has no DOM or markdown renderer.

### Compacted history

Current messages come from `get_messages`. `compactionSummary` becomes a non-prose
boundary with no raw summary text; `branchSummary` is omitted, matching Vue. Available
boundaries expose `history_available` and `history_loading` for control/retry presentation. `load_history`
walks the actual `get_entries` active ancestry from `leafId`, follows
`firstKeptEntryId`, and reveals one preceding layer without navigating branches.
Startup metadata may be the leaf; it need not be the last message or session-info entry.

Request/generation/identity fences reject stale history replies. Duplicate loads are
suppressed, failed requests release loading for retry, and cached entries avoid repeated
RPCs. Compaction refresh preserves a fresh streaming suffix without reintroducing hidden
pre-compaction messages. Hosts must keep boundaries out of prose, Copy and markdown
view caches, and retain reader position and unchanged source identities when prepending.

### Cosmetic saved preview

The host's existing bounded Pi-file reader supplies preview messages; it is not an RPC,
authentication or durability source. Tokens fence runtime/generation/identity/revisions.
Authoritative history matches full JSON Values and duplicate occurrence order through a
fingerprint bucket index; hashes/timestamps/displayed prose alone never prove identity.
The ledger retains one raw Value per occurrence, capped at 100,000 entries and 32 MiB
encoded JSON plus source strings (not an RSS ceiling). Overflow releases all overlap
proof and rebases cosmetic provenance after projection and on degraded restart; visible
rows remain. Authoritative RPC resets degradation. Normal indexing is linear in entries
and wire size, but deliberate hash collisions and unrelated controller/UI merge costs
are not promised linear.

### Scoped models

`State::models` retains the available catalogue; `State::selectable_models()` narrows
choices using an optional runtime scope or Vue-compatible `enabledModels` patterns.
Configured order and deduplication are preserved. Empty, invalid or unmatched scopes
fall back to the available catalogue. An unavailable or out-of-scope current model is
not injected into the choices; the host uses `State::model` for its display label.

Installed Pi 0.99.2 does not implement `get_scoped_models`. Rejection/timeout of that
optional probe does not block readiness or create a session error. The host must read
patterns through existing local/remote settings services, using a fresh token after
startup, identity replacement or model refresh. Stale completions are ignored. This
controller never opens settings, authentication or transcript files.

### Effects and durability

- `Send(Value)`: route to this controller's captured runtime/generation; the host must fence queued async writes too. Strict JSONL framing belongs to the transport.
- `RegisterSession { runtime_id, generation, id, path, name }`: register this **captured** identity in host storage, never select a UI row. Treat the effect as a storage instruction, not evidence that the write succeeded; retry storage failures in the host using the captured effect.
- `IdentityChanged { previous, current }`: runtime rebinding only; **no forced UI selection**. The host decides how to present/adopt/retire registry rows. Same requested path under a different ID yields session-local unsaved-source feedback, not silent substitution of the old registry identity.

A completed assistant `message_end` dispatches an identity-scoped `get_state` ordering barrier after Pi's synchronous append/flush. An exact successful barrier can register even empty completed output; fallback after barrier failure requires meaningful completed text/thinking/named-tool proof. Start-only output, aborted/error partial output, user-only history, notifications and pathless identities are not proof. Captured meaningful completion survives barrier failure, successor adoption and generation restart; predecessor handoff keeps original ID/path/name/proof generation without hydrating the successor or granting it durability. Existing host storage validation remains mandatory. Registration dedup means effect handoff, not successful persistence; the host retains exact failed tickets across restart and acknowledges storage separately. IDs namespace runtime + generation + increasing request sequence; superseded/unknown/stale replies cannot hydrate current state or acquire a newer command incarnation's error ownership.

Queue messages use real `prompt` with `streamingBehavior: "steer" | "followUp"`, after serialized acknowledgements of `set_steering_mode: all` and `set_follow_up_mode: one-at-a-time`. **Pi persists these account defaults**, affecting other processes; they are not restored. Snapshots come only from `queue_update`. Clear acknowledges actual `clear_queue` without erasing a newer snapshot. Stop does not clear or replay queues.

## Exact first-slice limitations

This is a working protocol core, **not TS runtime parity**:

- History supports current `get_messages` and layered pre-compaction reveal through active-branch `get_entries`. It does not provide branch/fork/navigation, general progressive history loading, or stable durable Pi entry cursors. Compaction success requests a fresh message projection; this remains narrower than the full TS history/replacement lifecycle.
- Timestamp/occurrence reconciliation assumes actual Pi message timestamps and append-order current history. Synthetic/malformed messages without timestamps, context rewrites, abandoned branches and compaction can change row IDs. Empty/image-only user messages have no image UI. Recognized skill envelopes expose a borrowed `Entry::skill()` projection while retaining exact source text; malformed envelopes remain ordinary user text. Host presentation is separate from controller storage.
- Tool execution events and final assistant tool calls are projected. `toolcall_*` argument streaming is not displayed before the final tool-call/message or tool execution start. Provider-level `done`/`error` updates rely on ordinary Pi `message_end`. Tool/custom renderers and image results are not implemented. Nested tool execution rows may disappear on authoritative hydration because Pi does not store nested calls as independent messages.
- Replacement detection includes lifecycle reads and finite delayed identity probes (150/450/1,000/2,000/3,200ms), with bounded materialization verification (250/750/1,500ms) and command-incarnation/serial fences. Completed predecessor intent survives failed reads/adoption/restart and exit handoff. Hosts own source recovery, archive mutations, storage receipts, idle eviction and explicit reconnect. Events for an undiscovered successor may transiently project under the predecessor; no event-carried identity or general live-suffix transfer is invented.
- Loaded history or cosmetic preview does not itself emit registration; registration requires a live assistant completion with an exact successful barrier, or meaningful captured completion proof handed off by bounded reconciliation, adoption or exit preparation. Rename uses correlated `set_session_name` requests, whitespace-normalized names bounded to 240 UTF-16 units, optimistic rollback and authoritative `SessionNameChanged` effects. The host persists those names and owns archive/restore mutations; delayed durability registrations must not overwrite a newer name. `durable` means protocol proof, not successful host registry persistence.
- Extension select/confirm/input/editor/notify/set_editor_text are handled. TUI statuses/widgets/titles/custom components, extension shortcut/flag/autocomplete controls are intentionally ignored. Dialog requests without Pi timeouts are user waits, not forced 10-second expiries. No host dialog-answer draft persistence API; the shell owns answer controls/drafts. Transport failure drops dialogs; reconnect does not replay responses.
- No reconnect, process spawning/killing, JSONL byte framing, SSH, ownership fencing, storage, telemetry, retry policies, or UI-selection behavior. Hosts start a new generation after reconnect; never replay a prompt automatically. Late callbacks must be fenced by the host. Active provider/tool hangs are not detectable without host health checks. Stop timeout is feedback, not proof Pi stopped.
- Errors remain reviewed local copy, not a provider-specific error classifier. Auto-retry events expose bounded, content-free retry state; host Retry Pi/fallback and retry presentation do not imply controller reconnect or automatic prompt replay. Local notices retain source identity/type through hydration, and the native host renders real prose/accessibility rather than surrogate text. This is not TS's fully anchored notice/compaction model. Transcripts/model catalogs are in-memory and not size-limited.

## Host presentation and service boundary

The native host now retains inline input/select/confirm and explicit multiline editor
controls with exact draft/undo/IME ownership; readonly accepted responses remain
transport-owned until the write settles. Embedded prompts use natural transcript
reader height; standalone dialogs keep their bounded scroll owner. These are host
contracts, not new controller dialog variants or a parallel extension runtime.

File classification, document-relative resolution, exact path Copy and bounded local/SSH
snapshot preparation belong to the host/shared services, not this RPC crate. Source owners
remain distinct from the native 128-view presentation cache. Authenticated SSH and
PDF/audio/video acceptance are not established by controller or fake-based tests.

## Verification

The workflow correction passed **155 controller tests / 1 ignored opt-in canary** and
**746 GPUI tests / 1 ignored**, strict all-target/all-feature native Clippy, static tooling,
eight package/version tests and **13 editor integrations**. It covers admitted preflight
beyond the ordinary deadline, transport loss after an idle state read, exact error
acknowledgement and preservation of uncertain writes. These are deterministic fixtures,
not execution of private workflows. Earlier unchanged-source gates remain shared core
**82 / 1 ignored**, canonical-path Tauri **16** and root frontend **894** plus static checks/build.

Both current installed-Pi native modes fail the unchanged five-second busy-close gate
with an inactive window, unavailable Close and no lifecycle callback. Cause is unproven;
no activation repair was added. Fresh ten-case visual comparisons at two scales are
representative, not whole-UI/packaged/SSH certification. Actual-label descender and
horizontal-overflow gates pass; remaining selection-paint differences are disclosed in
[APP.md](../../gpui/APP.md).

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
