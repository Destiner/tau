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
- Cold saved-session opens reserve the main pane immediately and show a centered
  loading indicator after 200ms. Fast hydration and new blank sessions do not flash it;
  selection/start/identity fencing cancels stale timers and startup errors reveal Retry Pi.
- Collapse/reorder update optimistically with revision-fenced persistence and rollback.
  Selection and new-session metadata writes no longer rescan every project's histories;
  registration/import merge returned rows/projects instead of reloading the workspace.
- Retained composer/transcript entities, full-height empty composer, model/effort
  choices, visibly rendered prompts/streaming prose/activity, Stop, queue submission
  and clear. While working, Enter or Send queues steering; Cmd-Enter queues a follow-up.
  Shift-Enter inserts a newline. When idle, Cmd-Enter sends an ordinary prompt.
- Typing `/` at the start of the composer opens Pi's session-owned command catalogue.
  Fuzzy filtering, source labels, clamped Up/Down navigation, pointer choice and Escape
  match Vue. Tab completes the active name without sending; Enter sends it once.
  Completion is undoable, IME keeps marked-text keys, and Shift-Enter remains a newline.
  While streaming, extension commands cannot submit; prompt/skill commands use the normal
  queue path. The bounded popup flips below the first line in the full-height composer.
  Command revisions avoid reparsing an unchanged catalogue on every streaming update.
  A known extension command admitted to the host transport retains its correlated owner
  while Pi awaits preflight or UI; the ordinary ten-second message deadline cannot invent
  a confirmation failure. Real rejection or transport loss still settles ownership, and
  an intervening idle state read cannot erase an unresolved write's uncertainty.
- Cmd-K command palette with real session/project/model/effort pages, supported actions
  and shortcuts, search, keyboard navigation, and modal-aware focus restoration.
  Session switching keeps shortcuts on the mounted shell while the destination paints;
  Cmd-P → switch → Cmd-K works without clicking or reactivating. Warm-session navigation
  returns to the retained composer (or its extension prompt); a newer pointer focus owner
  or overlay wins over the delayed handoff. A starting composer uses shell focus, then
  receives focus when ready unless another surface owns it. Failed pending operations
  remain retryable through Cmd-K → Retry Pending Operations; there is no global bottom
  retry strip. Removing that layout does not acknowledge or discard pending work.
- Leaving a truly empty, unregistered session retires its UI/runtime without deleting
  Pi files. Pending reads defer the decision; only explicitly abandoned candidates are
  revisited. Drafts (including whitespace), Pi-owned history, queued/recovered work,
  extension prompts, unresolved writes/reconciliation and captured registrations prevent
  retirement. Runtime notices and an acknowledged preflight failure alone are not saved
  conversation content. Dismissing feedback acknowledges its exact error occurrence;
  reselecting cannot replay it, and an old dialog cannot clear a newer error or write fence.
- Empty-session input uses a 4px internal top inset, 12px above the previous placement.
  Outer composer padding, compact input and footer position are unchanged. Scrolling plain
  textareas clip glyphs, selections and carets to their padded viewport, including at the
  multiline cap; single-line descender ink and retained editor state remain unchanged.
- Model choices use Pi's scoped-model settings for local or remote sessions, following
  Vue's matching/order/fallback behavior. An out-of-scope current model remains a
  display label, not an extra selectable option. Provider-grouped, searchable menus
  scroll within the viewport. Palette rows show model names without ID subtitles;
  thinking labels distinguish Max from Extra High.
  Choice revisions avoid rebuilding or serializing the catalogue during streaming.
- Thinking/tool adjacency uses Vue's compact spacing; tool headers include argument
  summaries such as `read  gpui/APP.md`. Multiline/heredoc arguments normalize whitespace
  only in the single-line, bounded header; raw arguments and results remain unchanged.
  Expanded details have retained, draggable scrollbars. Consumed vertical wheel movement
  stays inside the reader; fitting content and outward boundary gestures reach the transcript.
  Diagonal gestures retain their actual vertical component, while purely horizontal input
  cannot become vertical motion. Offsets survive updates/cache eviction. Short thinking
  readers do not autoscroll during an in-bounds horizontal text drag. Raw readonly results remain
  selectable with exact Copy, including whitespace and terminal newlines. The transient Working/Stopping tail follows
  Vue's assistant/compaction rules and is excluded from transcript data and Copy.
- Unnamed sessions derive `/skill:name` from a well-formed first skill invocation across
  header, sidebar, archive and palette, including saved previews/reloads. Explicit Pi or
  saved names take precedence. Shared parsing never rewrites transcript source or persists
  a derived title as a Pi name.
- Click-to-rename saved sessions through Pi, with bounded names, rollback and registry
  persistence. The title hover/focus wash is 20px high, matching Vue, with a 60ch cap
  and clipped long titles; the rename editor and title baseline are unchanged. Archive idle saved sessions; browsing archives does not restore them,
  and Restore is explicit. Drafts and session files are retained. Archives are
  virtualized, globally sorted within local-calendar groups and include project/model
  subtitles. Sidebar projections are invalidated at data/status boundaries rather than
  rebuilt for every text chunk; relative times and groups also age while idle.
  Saved workflow predecessors and replacements have distinct held-row identities, even
  when they share a runtime. Only an empty session's first materialization transfers
  its runtime-backed row identity.
- Draft/unread indicators, rich session status/message/time tooltips, SSH project
  alias/directory tooltips, and animated edge-reveal drag handles. Long SSH aliases and
  paths wrap within the compact tooltip and viewport rather than overflowing an intrinsic
  text-width box; full source text remains intact. Rich session titles retain their separate
  Markdown layout. Compact/rich/queue/first-run variants keep their distinct styles.
  First-run tooltips use measured button bounds, a 450ms delay and bottom placement.
  Row exit immediately
  hides pointer-only archive actions, including rapid motion and native window exit.
  Native deactivation mutes only Vue's selected live/archive rows and archive-view toggle;
  it does not dim text, editors or the body-ported palette.
- `iddqd` outside text fields unlocks the persisted shared admin/telemetry gate and
  retained issue reporter. Reports use the existing local feedback service, with an
  unchecked session attachment option. Palette reporting is available only in admin mode.
  Accepted report writes settle even when hidden; no timer invents a retryable failure.
- Extension input/select/confirm and explicit multiline editor requests use retained
  native controls inline in the transcript. Embedded prompts have natural height and
  the transcript owns reading; standalone dialogs retain bounded scrolling. Keyboard
  option reveal is separately fenced and does not let editor selection/IME move history.
  Active options paint Vue's hover wash as well as the accent stripe; accepted/readonly
  responses retain their highlight without allowing duplicate submission.
  Drafts, marked text, undo and readonly pending-response ownership survive updates.
  Notifications render actual prose with accessibility on the rendered leaves. Runtime
  notices retain their occurrence position through RPC hydration; compaction translates
  live notice anchors into the replacement tail without inventing persisted Pi entries.
- Application menus without a Settings surface or Cmd-comma shortcut, owned folder
  chooser, guarded close/quit and draft persistence. Closing the last production
  window quits only after confirmation (when busy), all-draft persistence and
  runtime shutdown acknowledgement. Closing one of multiple windows keeps the app alive.
- Compaction summaries render as non-prose dividers; branch summaries are omitted.
  Available dividers reveal an earlier active-branch layer through Pi's actual
  `get_entries`/`firstKeptEntryId`, with keyboard/pointer admission, retry and stale-reply
  fences. Prepending retains the current reader/source identities; dividers do not use
  the 128-view cache or become Copy sources. No alternate session format is introduced.
- Pending user bubbles use 0.88 opacity until Pi acknowledges them, without replacing
  their source/selection identity. Busy queue chips dim, Clear stays visible but disabled,
  previews remain usable, and only genuine errors show feedback—not Updating queue….
- Palette titles/details clip independently with reserved shortcut space; logical key
  tokens keep Ctrl+Tab and other modifier chords readable. Its retained search editor uses 13px type. Selector glyphs are smaller without shrinking hit areas.
  Shared light/dark scrollbar styles apply to editors, transcript and other scrollers.
- Generated prose file paths and explicit Markdown destinations preserve immutable
  source ownership, decode explicit percent spelling once, and resolve document-relative
  references in their document context. Copy Path and Copy Full Path preserve their
  respective written/resolved semantics, including literal POSIX backslashes. Local and
  SSH previews use bounded snapshot services with stale-owner/cancellation fences.
  Delayed Opening…/failure feedback uses the roomier Markdown surface, actual activated
  link-occurrence bounds (including wrapping), a 6px bottom gap and an 8px viewport clamp.
  Pointer and keyboard activation share those source bounds; context menus retain their
  pointer anchor. Non-inline linked images keep their existing activation fallback.
  File viewers overlay the full app viewport, including the sidebar and header, without
  reflowing the underlying shell. This is not authenticated SSH acceptance. Text/Markdown/code and admitted images are
  supported; PDF, audio and video acceptance is not claimed.
- One-command menus keep natural row height; selector chevrons follow Vue's glyph,
  and available compaction dividers expose the source-faithful hover affordance.
- First-run Pi fallback and Retry Pi preserve draft/workflow ownership. Version controls
  show the running version. The application-menu Check for Updates… opens that popup:
  native updates are unavailable and its primary update action is disabled, not simulated.
- Bounded presentation cache (128 views) and incremental changed-row projection retain
  source owners independently of evictable views. Streaming does not hash or clone
  unchanged historical text.

### Reconciliation, cleanup and commands

- Delayed identity probes, command-incarnation fences and bounded materialization
  verification preserve completed predecessor proof across replacement/restart.
  Registration uses captured identity/generation and existing storage validation;
  protocol durability is not a storage acknowledgement. Uncertain work is never replayed.
- Cold saved history can paint a cosmetic, cancellable preview from the existing Pi
  file through the shared 64 MiB reader. It grants no readiness, authentication,
  registration or durability. Authoritative RPC reconciles exact wire occurrences,
  not matching prose alone. The proof ledger is bounded to 100,000 rows / 32 MiB encoded
  JSON plus source strings; overflow abandons overlap proof and rebases cosmetic IDs,
  without dropping visible history. This is not a whole-UI linear-time guarantee.
- Six eligible inactive runtimes remain warm. Working, provisional, pending-read,
  queue, dialog, registration and recovery owners are excluded from idle release.
  Exact physical-stop receipts disconnect the stopped generation despite intervening
  work; newer generations survive old receipts. Only unchanged idle local sessions
  with a registered exact source restart automatically; remote/interfered sessions
  require explicit reconnect, without replay.
- Remove Project removes the Tau registry entry only, never project or Pi files.
  It captures runtime cleanup ownership, prunes shell projections after the storage
  acknowledgement, and retains failed operations for explicit retry.
- The palette covers the 36 Vue command identities with documented scope exclusions,
  plus native cleanup retries. It includes canonical read/unread, mounted sidebar/
  transcript focus, unsent review, available compaction-history reveal, combined project
  opening, explicit remote reconnect and nonwrapping adjacent-session navigation in
  displayed sidebar order. Navigation can preserve a session-owned extension prompt;
  unsafe edits and global modal conflicts remain blocked. Settings is intentionally
  absent; update status is presentation only, not an updater backend.
- Input writer failure is additive Tau transport `input_closed`, not Pi RPC or an
  output exit. Same-generation final output remains admitted while new work is blocked.
  Tauri ignores the additive kind and retains its existing write-failure feedback.
  Shutdown Save uses one acknowledged `stop_many` batch with one three-second drain
  budget, preserving successful receipts and retaining failed cleanup owners.
- Remote cleanup uses the accepted simple policy: natural SSH status 0–254 is proof
  under OpenSSH's remote-command-status assumption; status 255 or local kill is not.
  Uncertainty stays sticky and blocks replacement/reconnect and normal Quit. There is
  no override button or external-cleanup recovery workflow.

General media, authenticated SSH, broader extension coverage, release packaging and
production cutover remain unverified. Tree/branch navigation and destructive saved-session
file deletion are not missing Vue features in this audit; do not treat them as parity TODOs.
See [controller limits](../crates/tau-session/README.md).

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

Exit drains `prepare_exit` completed-output intents, exact source recovery and retained
storage/cleanup retries before shutdown Save. New completed proof during Save reopens
that drain. Save/Stop success is not approval: a worker FIFO ExitFence receipt, every
registration/recovery acknowledgement, and final revision/draft/source-binding checks
must settle first. Changed context requires another Save. Deadlines are finite vetoes,
never fabricated receipts; cancellation clears exit ownership without discarding drafts
or pending storage proof.

Native-app acceptance no longer closes/reopens the last production window. It preserves
the palette/remote, assistant/tool, rename/archive and session-switch sequence, cancels
a busy last Cmd-W with root/editor/draft/focus assertions, completes the extension, then
finishes with last Cmd-W. The harness requires completion-gated app exit and every
installed-Pi child exit acknowledgement before harness process-group cleanup. Generic
host reopen coverage remains separate. Historical close/reopen results below predate
this production policy. Both modes passed this policy at the historical 10 October 2026 checkpoint;
the current native acceptance failure is recorded below.

## Behaviour parity audit

The focused Vue/JS audit on 11 October 2026 checked sidebar, composer, selector,
rename/palette, extension-confirm and queue interactions against production sources:

- `ProjectSidebar.vue` / `session-order.ts`: saved rows keep Pi identity through cold
  startup, rather than becoming new runtime-keyed rows on first selection. List hover
  captures each project's membership/order; arrivals stay hidden until pointer exit,
  including in held-empty projects. Real removals close gaps; phantom materialization
  retains its source association and captured outgoing IDs win during replacement.
  Footer/titlebar hover is outside this hold; native deactivation releases it.
- Sidebar archive controls have the delayed action tooltip and shortcut. Measured
  overlapping action/row hit areas choose one tooltip owner. Archived rows have details
  tooltips; read-only header titles do not advertise Rename.
- Empty-runtime cleanup has separate pending and failed ownership. Accepted cleanup
  never paints Retry Save; genuine enqueue/worker failures offer retry. Cleanup still
  gates shutdown and successful acknowledgements clear only its owned error.
- `ComposerBar.vue` / `useTau/composer.ts`: empty or whitespace Enter/Cmd-Enter does
  nothing, even while working. Stop remains an explicit button or Cmd-period action.
- `useTau/settings.ts`: pending model/effort writes disable and close both selectors,
  not draft editing. The controller also rejects stale concurrent/busy setting actions.
- `state.ts` / `runtime/queue.ts`: compaction blocks Stop and queue admission without
  disabling draft editing. A queue gesture crossing compaction is cancelled and its
  text retained; late mode replies cannot resume it after compaction ends.
- `SessionHeader.vue`: opening the palette by keyboard or pointer suspends rename,
  not commits it. Escape restores its editor/text; switching sessions cancels it.
  Ordinary outside clicks still commit without stealing the clicked control's focus.
- `ExtensionDialog.vue`: Cmd-Enter confirms Yes, Cmd-Shift-Enter confirms No, independent
  of highlighted row; ordinary select Enter, busy and composition rules are unchanged.
- `MessageQueue.vue`: Clear All is disabled while queue operations are pending;
  rejection/timeout releases admission without losing items. The later transcript/UI
  correction below deliberately replaces generic busy text with quiet dimmed chips.

This is a focused interaction audit, not a claim that all runtime/recovery parity is
finished. Settings remains intentionally absent; general media support, authenticated SSH,
release and cutover gates remain as documented above.

## Current verification boundary

The archive/selection/retry-layout corrections passed **753 GPUI library tests / 1 ignored**,
**155 controller / 1 opt-in canary ignored**, **13 editor integrations**, eight package/version
checks, strict Clippy/static checks and the offscreen composer regression. A normal-feature
optimized arm64 release build passed last. Actual Root regressions reproduce the historical
held-row alias and stopped-release gesture defects; they do not identify the exact live
incidents from telemetry. Text/code/emphasis/link triple-click selects one logical paragraph
with exact source/Copy; inline-object paragraphs preserve their existing visual-row behavior.
Outside controls cannot initiate a reader drag, and release or button-free motion ends it
without erasing a completed interval. No new desktop or broad paired-visual campaign is claimed.

The preceding skill-title/composer corrections passed **746 GPUI library tests / 1 ignored**,
**82 Core / 1 ignored**, **155 controller / 1 opt-in canary ignored**, **16 Tauri compatibility
tests**, strict all-target/all-feature Clippy, formatting, native lint/typecheck and eight
package/version tests. All **13 editor integrations** and the new offscreen composer
pixel/mask regression pass. The latter reproduced actual glyph escape before Base34,
then passed with the same containment contract after preparation. A normal-feature
optimized arm64 release build passed after renderer validation.

The preceding workflow/tooltip correction passed **742 GPUI library tests / 1 ignored**,
**155 controller tests / 1 opt-in canary ignored**, strict all-target/all-feature Clippy,
formatting, native tooling lint/typecheck, **13 editor integrations** and eight
packaging/version tests. A normal-feature `tau` build passed after the feature-enabled
visual checks. Deterministic coverage includes admitted extension preflight,
transport loss after idle reads, exact error acknowledgement, empty-session retirement,
active prompt wash, source-owned feedback placement and long-tooltip containment.

That checkpoint's actual-App comparison captured **56 originals / 28 Vue/native pairs**: seven states,
both themes, 1×/2×, plus eight held-workflow/busy-response smokes. Long SSH locations remain
complete inside the 260px surface; first-run hints sit 6px below their actual icon. Native
and WebKit still differ in text wrapping/rasterization, some prompt spacing and focus cues;
this is not pixel-perfect parity. Vue changes its hover cursor on a disabled option while
native retains it, but both keep the same pending response and admit no duplicate write.
The reported private `/implement` workflow was not executed; these fixes use Pi's public
contract and deterministic fixtures, not private extension/authentication data.

The earlier interaction-polish checkpoint passed **718 GPUI library tests / 1 ignored**,
strict all-target/all-feature Clippy, formatting, native tooling lint/typecheck,
**13 editor integrations**, eight packaging/version tests and controller
**143 / 1 opt-in canary ignored**. Separate unchanged-source passing gates remain shared
core **79 / 1 ignored**, canonical-path Tauri **16**, and root frontend **894** with static
checks/build. A normal-feature `tau` build passed after the feature-enabled checks.
These gates do not replace native lifecycle or packaged acceptance.

**The last installed-Pi native automated acceptance was not green in either mode.**
Those credential-free and model-backed runs failed the unchanged five-second busy-close gate:
the owned window is inactive, Close is unavailable, and no lifecycle callback runs.
Deactivation's source cause is unproven; an external actor is not established. No
activation repair, repeated key or widened deadline was added. Earlier free/model passes
are historical; harness child cleanup is not successful production close.

Subsequent user manual checks passed close/cancel, draft persistence, streaming/session
switching/queues, history management, macOS input and ordinary extension prompts.
The user subsequently reported complex workflow execution/resumption and SSH working
while using GPUI as the main app. These are accepted manual checks for the cold-turkey
cutover, not automated authenticated-host proof. Additional compaction, automated busy-close
and existing-profile acceptance campaigns are explicitly deferred, not passing gates.
Media expansion is excluded from rewrite completion. The requested repository order is
remove Vue/Tauri, remove temporary migration/bridge tooling, harnesses and docs, then open
a PR for review. Do not automatically merge. Preserve durable native tests and unrelated
local work. Native release automation, updater implementation and canonical CI remain
engineering work, not implied by cleanup. Do not reopen broad acceptance campaigns
without a concrete regression.

The focused interaction-polish comparison captured **104 original images / 52 Vue/native
pairs**, covering 13 states in both themes at 1×/2×. Actual Root interactions verify reader
thumbs/edge handoff, stable held selection, picker labels, pointer-exit timestamp restoration,
real inactive selection washes and full-window preview coverage. The headless WebKit tool
thumb is not visibly painted, so scrollbar raster equivalence is not claimed. Existing font,
inset, shadow/caret and small picker-state differences remain disclosed in the review.

The preceding finalisation's production Vue/native captures cover ten representative cases in both themes at
1×/2×, using real Root and actual dark rendering. They do not establish whole-UI parity.
Selection retains a one-RGB-channel light difference and a one-CSS-pixel Retina band
coverage difference. All **452 captures** (208 Vue, 204 native, 40 typography controls)
are source-frozen. Actual-label glyph controls confirm preserved descenders, unchanged
non-descenders, matching fixed/x-only reference pixels and genuine horizontal overflow
in the unbounded negative control. Twelve literal selected-string pairs pass; differing
drag coordinates are not treated as identical ranges merely because frame IDs match.

SourceSelection survives dehover, fractional gaps and three held frames by resolving the
nearest painted run; normalized CRLF display still copies original source exactly.
Font-metric selection paint uses actual fragment metrics at both body scales without
changing hit testing/layout. Single-line truncation constrains width while preserving
line height and glyph ink; enclosing masks still apply. Source owners remain independent
of the **128-view** cache. These contracts do not certify packaged/SSH acceptance.

The earlier Tauri updater initialization failure reproduced on baseline under macOS's
symlinked `/tmp` path; canonical-path checks pass. The extra upstream framework Core run's
two historical spring failures remain a separate boundary.

## Historical verification checkpoints

Counts and comparisons below describe their own earlier source checkpoints, not the
current suite or current visual acceptance.

The command/header correction passed **469 library tests** (one ignored timing
benchmark), **13 editor integration tests**, and **64 controller tests** (one opt-in
installed-Pi canary ignored). Formatting, GPUI lint/typecheck, six packaging tests and
strict GPUI all-target/all-feature plus controller Clippy passed. Real Root regressions
cover authoritative command hydration, Tab/Enter/pointer routing, session isolation,
streaming admission and a quiet first-send pending state. Both credential-free and
model-enabled native acceptance exercise the installed Pi catalogue and Tab completion
before the existing extension/model/tool/Stop/session/shutdown sequence.

Six fresh light/dark Vue/native/diff pairs cover the command popup above/below the
editor and the title hover. They use production components, fonts and CSS import order;
40.5px described-command rows, alpha-composited cursor wash and 20px title hover are
measured and painted. The intentionally raised native empty-composer first line places
its below popup 12px higher than current Vue; Base Positioner retains a 0.5px above-popup
translation difference. Font rasterization also differs. Initial incorrect normal-leading,
CSS-order and unsnapped geometry assumptions remain in the local evidence, not hidden
by coordinate corrections. This is focused comparison, not full application parity.

Current packaging calls the bundle **Tau.app**, with Tau display/application names,
while preserving `com.destiner.tau.native-preview`. Packaging rejects existing targets,
symlinks and system/current-user Applications destinations. It does not replace installed
Tauri Tau. The preview still has no native updater/feed. The current **Check for Updates…**
menu opens Version's unavailable panel with its primary action disabled; it is not
wired to the incompatible Tauri updater. Local ad-hoc packaging remains
separate from signed/notarized distribution and production cutover.

The previous transcript/UI correction passed **450 library tests** (one ignored timing
benchmark), **13 editor integration tests**, and **62 controller tests** (one opt-in
installed-Pi canary ignored). Formatting, GPUI lint/typecheck, five packaging tests and
strict GPUI all-target/all-feature plus controller Clippy passed. A fresh arm64 debug
`Tau Native.app` passed packaging/linkage and ad-hoc signature verification; it is not
notarized and this does not establish packaged local/SSH workflow acceptance.

Both credential-free and model-enabled native acceptance passed with Pi 0.99.2.
A synthetic Pi v3 fixture under the isolated profile is opened through the real worker
and measured sidebar. Actual `get_messages`/`get_entries` replies prove summary exclusion,
prior-layer reveal, current reader bounds/source retention and return to the Unicode
draft, alongside existing workflows and guarded shutdown. These tests do not read
production sessions or prove that compaction explained the user's entire malformed
transcript report. Root tests additionally cover exact delayed loading, unavailable/
busy divider semantics, pending acknowledgement, one-line tools and optimistic mutations.

Eight fresh light/dark pairs import actual production Vue and native components inside
Base Root: activity, palette, scrolled composer and pending transcript/queue. Paired
images/diffs were inspected; pending bubbles and wheel-revealed themed thumbs are asserted
painted. Smaller native chevrons (6px versus Vue's approximately 8.485px rotated extent)
and quiet busy queues intentionally differ from Vue. Text rasterization/rounding and
platform scrollbar appearance retain visible differences; this is focused evidence,
not a full pixel-parity campaign or release certification.

Intermediate failures remain recorded: an invalid generic accessibility role on passive
dividers, a stale seeded-leaf wire assumption, inactive keyboard/handle-lifetime test
fixtures, and incomplete visual-wrapper sizing/activation. Fixes retain real ancestry,
initial fixture setup, painted-content checks and unchanged native deadlines. At that historical checkpoint a tool-detail selection experiment was reverted after
losing a terminal newline. Current retained readonly exact-copy support supersedes
that limitation. No dependency-cache patch or production data migration was added.

The earlier behavioural audit passed **422 library tests** (one ignored timing benchmark),
**13 editor integration tests**, and **52 controller tests** (the opt-in installed-Pi
canary remains ignored). Formatting, GPUI lint/typecheck, strict all-target/all-feature
Clippy and the native build passed. Production-root tests measure four cold and four
warm sidebar selections, exact row/lower-project bounds, hidden arrivals, materialization
and replacement; a legacy runtime-ID negative control breaks the same painted oracle.

Both credential-free and model-enabled native acceptance pass real hovered session
creation with unchanged row bounds, hidden arrival until natural pointer exit, empty
cleanup without Retry Save, existing keyboard/admin/draft workflows and guarded shutdown.
The model run additionally measures the archive tooltip above its action and retains
assistant/tool/Stop/rename/archive/restore coverage. Its owned renderer image was
inspected; no fresh full paired Vue/native visual campaign or authenticated SSH claim.

Intermediate evidence is retained: the first tooltip implementation let overlapping row
hover replace the action timer; new fixtures initially failed to settle all bootstrap
reads; the older native sequence tried to click an arrival while still intentionally
held hidden. Corrections preserve the 450ms tooltip contract, real pointer exit and
unchanged acceptance deadlines. Build/check command timeouts were resumed separately;
no application activation or focus repair was added.

The session-switch follow-up passed **393 library tests** (one ignored benchmark),
**13 editor integration tests**, formatting, GPUI lint/typecheck, all-target/all-feature
strict Clippy and the actual native build. Both credential-free and model-enabled native
acceptance passed actual Cmd-P/search/arrows/Enter → immediate Cmd-K/Escape, empty-session
runtime cleanup acknowledgement, retained drafts, admin reporting and guarded shutdown.
The model variant retained assistant/tool/Stop and rename/archive/restore coverage.
The empty first-line bounds were measured at composer-area top + 10px and the owned
native image was inspected. This requested 12px inset reduction intentionally differs
from Vue's older empty-textarea override; no full paired visual campaign was performed.

Negative controls reproduce the old palette handoff, false self-containment membership,
and prepaint focus race. Initial native runs also caught missing New Session focus and
an immediate-shortcut gap before the destination paint. Both are fixed with owned focus
handoff, not test refocusing/activation, retries or widened deadlines. The initial static
batch timed out during all-target checking; resumed strict checks passed.

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
`Tau.app` preview can be built using [PACKAGING.md](PACKAGING.md); it neither
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
