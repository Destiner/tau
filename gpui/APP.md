# Native Tau application

`src/bin/tau.rs` launches the canonical application. `tau-core` owns workspace,
profile, process and SSH services; `tau-session` owns Pi RPC orchestration;
`tau-observability` owns shared preferences, feedback and diagnostics. Pi owns
extensions and transcripts. This app does not use a synthetic conversation runtime.

## Run and storage

From the repository root:

```sh
bun install --frozen-lockfile
bun run prepare
bun run dev -- --isolated /absolute/profile --project /absolute/project
```

The development launcher requires isolation. Direct Cargo invocation works after
prepare with `--manifest-path gpui/Cargo.toml --bin tau`. Isolated profiles persist,
with metadata under `tau/` and local sessions under `pi-sessions/`; they have no
synthetic seed. `--session <registered-id-or-path>` selects a registered session.
Isolation is not a tool/filesystem sandbox: Pi retains credentials, extensions and
access to the project. See [storage](../docs/development-storage.md).

A normal product launch without `--isolated` uses shared production storage. The
cutover is cold-turkey, not simultaneous old/native operation. Cross-process profile
locking is not certified. Pi files need no conversion; native preferences/drafts use
`native-ui.json`, and migration of old Vue drafts is not guaranteed.

## Interaction contracts

- Local/SSH project import, saved/new sessions, scoped model/effort choices,
  rename through Pi, explicit archive/restore and registry-only project removal.
  Removing a project never deletes project or Pi files. Archive browsing does not
  restore a session; drafts and transcripts remain intact.
- Retained composer and transcript entities preserve draft, undo, IME and selection
  ownership across updates and navigation. Cold history reserves space immediately;
  its loading indicator appears after 200ms. Hydration cannot steal newer focus.
- Enter sends when idle and queues steering while working; Cmd-Enter queues follow-up
  while working and sends normally when idle. Shift-Enter inserts a newline. Empty
  submissions do nothing. Stop aborts work, never clears queues. Compaction blocks
  Stop/queue admission without disabling draft editing; cancelled admission retains text.
- `/` opens Pi's session-owned command catalogue. Tab completes without sending;
  Enter submits once. IME retains marked-text ownership. Extension commands cannot
  execute from the busy queue path. Pending model/effort writes disable their selectors,
  not the draft. Pi remains authoritative for command and model discovery.
- Cmd-K palette and session/project/model/effort pages share guarded actions with
  menus and buttons. Escape closes only the topmost dismissible layer. Navigation
  restores a valid retained composer or extension prompt, without stealing newer
  pointer focus. See [keyboard inventory](../docs/keyboard.md).
- Sidebar hover holds displayed membership/order, hides arrivals until pointer exit,
  and closes genuine removal gaps. Row contents stay live. Adjacent-session navigation
  follows displayed native order, not an independent live projection. Native inactivity
  releases the hold and changes selection washes, not editor or transcript content.
- Inline extension input/select/confirm/editor controls retain answer drafts and
  pending-response ownership. Embedded prompts use transcript scrolling; standalone
  dialogs are bounded. Prompts and notices belong to their originating session.
- Rename is suspended by opening the palette, not blur-committed. Escape restores it;
  switching sessions cancels rename. Ordinary outside clicks commit without stealing
  the clicked control's focus. Confirm shortcuts choose Yes/No independently of highlight.
- Failures stay with their operation/session. Dismissal acknowledges only the displayed
  occurrence. Cmd-K → Retry Pending Operations retains actionable recovery; permanent
  remote cleanup uncertainty is not offered as a futile retry. No global retry strip.

## Transcript and preview contracts

Streaming preserves reader position and source identity independently of the bounded
128-view presentation cache. Expanded tool/thinking readers retain offsets and exact
readonly Copy, including whitespace and terminal newlines. Actual vertical scrolling
stays inside a reader until an outward boundary gesture hands it to the transcript;
horizontal input cannot become vertical motion. Editor glyphs, carets and selection
are clipped to their padded viewport without discarding editor state.

Compaction summaries are non-prose dividers; branch summaries are omitted. Available
dividers reveal earlier active-branch history through Pi's `get_entries` and
`firstKeptEntryId`, with stale-reply fencing and retained reader/source ownership.
Notices remain occurrence-anchored across hydration and compaction, never fabricated
as persisted Pi entries. Pending user bubbles and Working/Stopping presentation are
not substitute transcript data or Copy sources.

Links preserve immutable written source, decode explicit percent spelling once and
resolve relative destinations in document context. Copy Path and Copy Full Path keep
written/resolved semantics, including literal POSIX backslashes. Local/SSH previews
use bounded snapshots with stale-owner/cancellation fences and cover the full app
viewport without reflow. Text, Markdown, code and admitted images are supported;
PDF/audio/video acceptance and broader media expansion are not claimed.

Cosmetic saved-history preview uses the shared 64 MiB reader. It grants no readiness,
authentication, registration or durability. RPC reconciliation matches exact wire
occurrences. Its proof ledger is bounded to 100,000 rows / 32 MiB encoded JSON plus
source strings; overflow rebases cosmetic IDs without dropping visible history.
This is not a whole-UI linear-time guarantee.

## Runtime, recovery and exit

Every runtime/generation/request and command incarnation has a captured owner.
Delayed identity probes and materialization barriers preserve predecessor proof;
registration requires storage acknowledgement, not just protocol durability.
Uncertain prompts, commands or extension responses are never replayed. See
[Pi contracts](../docs/pi.md) and [controller limits](../crates/tau-session/README.md).

Six eligible inactive runtimes remain warm. Working, provisional, pending-read,
queue, dialog, registration and recovery owners are protected from idle release.
Only unchanged idle local sessions with exact registered sources auto-restart;
remote/interfered sessions require explicit reconnect. Truly empty unregistered
sessions retire only after owned reads/reconciliation settle. Drafts (even whitespace),
history, queues, extension work and durability/recovery owners prevent retirement.

Input writer failure is Tau transport `input_closed`, not Pi RPC or output exit.
Final same-generation output remains admitted while new work is blocked. Physical
stop receipts disconnect only the stopped generation. Remote cleanup accepts natural
SSH status 0–254 under OpenSSH's remote-command-status assumption; status 255 or local
kill is not proof. Sticky uncertainty blocks replacement/reconnect and normal Quit.
There is no override or external-cleanup recovery workflow.

Red close, Cmd-W and File → Close Window use the same guard; last-window close becomes
guarded Quit. Closing another window keeps the app alive. Cancellation or failed save
leaves the editor intact. Close approval never implicitly authorizes Quit: promotion
to last-owner Quit requires fresh guarded approval within the original finite deadline.

Exit drains completed-output proof, source recovery and storage/cleanup retries before
Save. Successful draft writes advance the durable baseline monotonically. Shutdown
uses acknowledged `stop_many` with one three-second drain budget and partial receipts.
Save/Stop success alone is not exit approval: FIFO ExitFence, registration/recovery
acknowledgements and final revision/draft/source checks must settle. Changed context
requires another Save; deadline expiry vetoes exit, never fabricates a receipt.

## Admin and distribution boundaries

`iddqd` outside text fields toggles the persisted admin gate. Telemetry and the retained
issue reporter are off by default and unlocked together. Reports await actual append
acknowledgement even when hidden; delays cannot invent retryable failure. Telemetry
is bounded, content-free and best-effort; see [observability](../docs/observability.md).

Settings is intentionally absent (including Cmd-comma); persisted theme choices remain
readable, otherwise appearance follows the system. Check for Updates… opens Version
with unavailable copy and a disabled primary action. There is no native updater/feed,
installation or rollback implementation. [Preview packaging](PACKAGING.md) remains
ad-hoc. Separate production packaging infrastructure for `com.destiner.tau` version
0.3.0 is implemented/in review, without a completed signed candidate or packaged
runtime acceptance. [Release safety](../docs/releases.md) owns the protected flow
and public-release blockers; a manual-update public first release is still undecided.
Candidate inspection never launches or installs the app and cannot prove runtime behavior.

## Accepted and deferred verification

User manual checks accepted close/cancel, draft persistence, streaming, switching,
queues, history management, macOS input, ordinary extension prompts, complex workflow
execution/resumption and SSH for this cold-turkey switch. They are not automated
authenticated-host or packaged-distribution proof.

The last installed-Pi automated acceptance failed the unchanged five-second busy-close
gate in both modes: the owned window was inactive, Close unavailable and no lifecycle
callback ran. Deactivation's cause is unproven; no activation repair, repeated key or
widened deadline was added. Historical passes and harness child cleanup do not turn
that failure into successful production shutdown.

Additional compaction, automated busy-close and existing-profile campaigns are
explicitly deferred, not passing gates or reopened blockers without a concrete
regression. General media expansion is excluded. Tree/branch navigation and destructive
session-file deletion are not promised features. Deterministic native/controller/core,
editor and Metal regressions remain the maintained default verification surfaces;
[CI/testing](../docs/ci-testing.md) distinguishes these from explicit OS/real-Pi tests.
