# GPUI rewrite: current status and delivery plan

Updated 13 October 2026. This supersedes the historical prototype checklist.
The requested delivery/decision window was **9–10 October**. The native preview remains
under active user review and correction; it is not a completed production cutover.
The remaining release/cutover gates below still require an explicit decision.

## Current boundary: first real native application implemented

`gpui/src/bin/tau.rs` now launches the production application in `gpui/src/app.rs`.
`tau` is Cargo's default run target; the older `src/main.rs` playground remains a
reference tool, not the application runtime. The real shell reuses accepted
chrome, transcript, composer, selectors, dialogs and native host adapters. It owns
workspace/session state, persisted preferences, retained editors and lifecycle requests; it does
not use synthetic projects, timers or fixture transcripts.

Framework-independent process, storage, profile and SSH services were extracted
into `tau-core` in `af98b6d`, preserving existing Tauri service compatibility.
`tau-session` supplies real Pi JSONL RPC orchestration; the shell (`63cba9c`) routes effects
through those shared services. Local and SSH backends are available, including
existing remote-project configuration and a new SSH connection/directory import UI.
**Automated authenticated real-host SSH acceptance remains unproven.** Ordinary SSH
appeared to work during subsequent user review. Pi still owns extensions and session files.

Implemented first-slice behavior includes local project import, existing/new
sessions, model/effort selection, prompt/stream/activity/Stop and queue controls,
extension input/select/confirm and custom output, owned folder
selection, drafts, guarded close/quit, and quit on the last window after persistence
and runtime cleanup. Generic host reopen remains separately supported. See
[APP.md](../gpui/APP.md) for launch and profile safety instructions and
[controller limits](../crates/tau-session/README.md) for exact protocol boundaries.

**This is an operational native slice, not whole runtime parity or cutover.**
Settings is intentionally absent: no panel, menu item or Cmd-comma shortcut.
Existing persisted theme choices remain readable; otherwise appearance follows the system.
Tauri remains the operational fallback. A standalone, locally ad-hoc-signed
**Tau.app** preview packages the actual GPUI executable (packaging introduced in
`4366e27`; earlier previews were named Tau Native.app), with a separate bundle identity
and verified metadata/linkage. Packaging refuses system/current-user Applications
locations and existing destinations, including symlink aliases. It has not replaced
Tauri or been certified for distribution. There is no Developer ID signed/notarized
GPUI release or GPUI updater. See [packaging](../gpui/PACKAGING.md).

## Current parity and verification boundary

Generated file paths, explicit decode-once percent destinations, document-relative
links and exact Copy Path/Full Path now use source-faithful ownership. Bounded local
and SSH snapshot services support text/Markdown/code and admitted images, without
claiming authenticated SSH or PDF/audio/video acceptance. Source owners outlive the
128-view presentation cache. Notices render actual prose and rendered-leaf accessibility;
readonly tool output retains exact selection/Copy. Tool details have retained draggable
scrollbars and consume actual vertical movement, handing outward boundary gestures to the
transcript. Short thinking readers retain horizontal text selection without autoscroll.
One-command rows have natural height, chevrons follow Vue, and compaction hover is source-faithful.

Warm-session navigation restores the retained composer or extension owner without stealing
newer pointer focus. Sidebar row exit clears pointer-only archive controls; native window
inactivity changes only Vue's selected sidebar washes. Palette shortcuts use logical key
tokens, model rows omit ID subtitles, and Max remains distinct from Extra High. Remote
picker labels use Go Back and Select with the full path. Runtime notices stay anchored
through hydration, including live arrivals during compaction. File viewers cover the whole
app viewport rather than occupying a sibling content column.

Inline extension editors retain native draft/undo/IME state. Embedded prompts use the
transcript's natural reader mode; standalone dialogs remain bounded. Explicit
`NativeDialog::new_editor` preserves the existing `DialogKind`/`DialogProps` API.
First-run fallback, Retry Pi and version controls are wired. Check for Updates… opens
the Version popup with unavailable copy and a disabled primary action. There is still
no Settings surface or native updater.

Completed runtime contracts include delayed identity/command reconciliation, predecessor
materialization proof across restart, captured registration retries, cosmetic saved-history
preview, idle release, registry-only project removal and staged exit storage/drain fences.
Six eligible inactive runtimes remain warm; work/provisional/read/queue/dialog/registration/
recovery owners are excluded. Only unchanged idle local registered sources auto-restart;
remote/interfered runtimes require explicit reconnect without replay.

Writer failure is additive Tau transport `input_closed`, not a Pi wire kind or output exit.
Final output remains admitted; Tauri ignores that kind and retains write-failure feedback.
Shutdown Save uses acknowledged `stop_many` with one three-second budget and partial receipts.
The accepted simple remote policy accepts natural SSH status 0–254 under OpenSSH's remote
command assumption, not 255 or local kill. Sticky uncertainty blocks cleanup, reconnect
and normal Quit; no override button or external-cleanup recovery workflow exists.

Palette coverage maps 36 Vue IDs with deliberate scope exclusions and native extras;
Settings is intentionally absent and update status has no updater backend. Cosmetic
preview uses the shared 64 MiB reader and an exact-occurrence proof ledger bounded to
100,000 rows / 32 MiB encoded JSON plus source strings; overflow abandons cosmetic ID
continuity, not visible history or authority. SourceSelection preserves original Copy
through normalized CRLF, dehover and nearest-run gaps held across three frames.
Font-metric paint changes no layout/hit testing; width truncation preserves line height
and bottom ink. Source owners outlive the 128-view cache. See [APP.md](../gpui/APP.md).

Interaction-polish checkpoint: **718 GPUI passes / 1 ignored**, strict all-target/all-feature
Clippy, formatting, native tooling lint/typecheck, eight package/version tests,
**13 editor integrations**, and **143 controller / 1 canary ignored**. Unchanged-source
separate passing gates remain **79 shared-core / 1 ignored**, **16 canonical-path Tauri**,
and **894 root frontend** plus static checks/build. The normal-feature native build also
passed after feature-enabled checks. These checks do not establish native lifecycle,
packaged or whole-product acceptance.

**Both current installed-Pi native modes fail** the unchanged five-second busy-close gate:
inactive owned window, unavailable Close, no lifecycle callback. Deactivation origin is
unproven, not proven external; no activation repair was added. Historical passes below
do not certify current close/cancel/shutdown. Subsequent user manual checks passed
close/cancel, persistence, streaming/switching/queues, history management, macOS input and
ordinary extension prompts. Compaction and complex extension workflows remain unverified;
the manual reports do not erase the automated gate's negative evidence.

The focused interaction-polish campaign captured **104 originals / 52 Vue/native pairs**:
13 states, both themes, 1×/2×. Real Root interactions include reader handoff/held selection,
pickers, timestamp restoration after pointer exit, native inactive selection and full-window
preview coverage. Headless WebKit's tool thumb is not visibly painted; no scrollbar raster
equivalence or whole-UI pixel parity is claimed. Existing raster/layout and small picker-state
differences remain disclosed.

The preceding finalisation's production captures cover ten representative cases in both themes at 1×/2× using
real Root/dark rendering, not the whole UI: **452 source-frozen images**, including 40
typography controls. Actual-label descender/non-descender and horizontal-overflow gates
pass, as do twelve literal selected-string pairs and natural paused dehover. Selection
still differs by one light RGB-channel and one CSS pixel of Retina band coverage.
Historical upstream Core spring failures remain separate from local native, packaged/SSH
and cutover acceptance.

## Historical evidence and limits

The following counts and comparisons are checkpoint-specific, not current acceptance.

- Command/header correction: Pi-backed slash command picker, fuzzy source-labelled
  choices, Tab completion without sending, exactly-once Enter, clamped arrows, pointer
  selection, Escape, retained-editor undo/IME and streaming admission. Command revisions
  fence catalogue replacement and avoid repeated deserialization during streaming.
  First-send pending feedback stays quiet. Title hover is Vue's measured 20px high.
  **469 library / 64 controller / 13 editor tests**, strict Clippy, formatting,
  lint/typecheck and **six packaging tests** passed; both native acceptance variants
  exercise real Pi command hydration/Tab alongside the existing workflows.
- Six fresh light/dark production Vue/native/diff pairs cover above/below command
  menus and title hover, with actual CSS import order and painted geometry. Initial
  normal-leading/CSS-order and pixel-snapping fixture errors were corrected and kept
  as negative evidence. Intentional 12px empty-inset difference, native Positioner's
  half-pixel translation and font rasterization differences remain disclosed. Comparison
  is focused, not full parity certification; temporary comparison machinery stays local.
- The app/bundle is now named Tau, without changing the separate native-preview identity
  or installing over Tauri. Check for Updates… now opens the unavailable Version popup; its primary update
  action remains disabled. The Tauri update feed is not a compatible native update path.
- Core extraction: **57 unit + 2 integration tests passed**, one explicit canary
  ignored. Existing Tauri compatibility tests and unsigned Tauri bundle passed;
  that bundle is not GPUI packaging.
- `tau-session`: **48 tests passed**, including model-choice revision/streaming cost,
  compaction presentation, scoped-model ordering/fallback/fences,
  tool summaries, rename correlation, rollback and name bounds; one opt-in installed-Pi
  canary ignored in the default run.
- Native credential-free acceptance passed with installed **Pi 0.99.2** in an
  isolated profile: command bootstrap, actual extension input/Enter/custom output,
  session switching and drafts, native menus, close/reopen, busy-close cancellation,
  restored dialog focus, and completion-gated quit/process cleanup.
- Reopen now presents a fresh per-window surface only after remount. The regression
  clicks actual measured controls and includes a negative control reproducing the
  stale first-paint listener failure. Targeted application tests: **12 passed**;
  application build and application-scoped strict Clippy passed.
- Command-only custom output remains ephemeral: credential-free acceptance is
  **not assistant streaming/durable-session proof**.
- The first optional model run failed the unchanged **60-second** compound gate.
  The test bug was identified: Pi `message_update` is delta-only, not
  `message_update.message.role`. The corrected recorder reads
  `assistantMessageEvent.text_delta`; acceptance still requires actual assistant
  text, assistant end, idle, on-disk durability and registry persistence. Corrected
  validation **passed**. It also passed subsequent reopen/switch, busy-close
  cancellation and gated quit cleanup.
  Earlier failed evidence remains; no deadline widening. This proves the small
  local assistant slice, not SSH acceptance or full runtime parity.
- Real model-issued tool/Stop acceptance passed (`6d947ff`): fixture tool
  start/partial, measured native Stop click, Pi abort acknowledgement, aborted tool
  end and settlement; partial output and unrelated draft retained. Built-in tools
  and discovered extensions stayed disabled. Populated queues and interruption
  during provider token streaming are not covered by this slice.
- Hovering during asynchronous session loading exposed a separate hidden-row bug.
  Membership now reconciles without discarding held order (`4eaf6ee`); old-code
  negative controls fail and genuine pointer regressions pass.
- Reported application regressions fixed: full-height empty composer, visible
  sent/streamed messages, bottom toolbar insets and unclipped title descenders.
  Production-root paint/selection regressions and inspected native Metal images
  now cover the composition that component-only tests missed.
- Saved-session header rename and archive/browse/restore are wired through Pi and
  existing storage, with identity/name-write fencing and preserved drafts. Real
  Pi acceptance passed these interactions; that extended run later hit an inactive
  window at Cmd-W and is not a full smoke PASS. A final credential-free native
  run passed close/reopen and shutdown.
- Cmd-Enter now carries follow-up intent in the submit event rather than a shell key
  listener; actual RPC regressions distinguish follow-up from steering and preserve IME.
- Workflow batch: command palette with real actions/pages/shortcuts; remote connection,
  directory browsing and serialized project import; local/remote scoped model choices
  matching Vue; compact thinking/tool spacing and argument details in tool headers.
  Production-root regressions cover geometry, rendered controls, focus, stale callbacks,
  import persistence and scope filtering. Remote imports merge the returned project,
  not a competing workspace snapshot. No authenticated SSH host was exercised.
- Native Cmd-K → remote form → cancellation/reopen passed with measured controls,
  actual editor focus/value and retained drafts. Palette/remote/tool-transcript Metal
  images were inspected. Initial new-test failures expected focus/value facts on wrapper
  snapshots; the corrected predicate uses the actual editor, without a deadline change.
  The optional model run passed assistant/tools/Stop and session actions, then again
  timed out at the later Cmd-W gate; no complete model-smoke PASS is claimed.
- Sidebar/admin batch: nested tool scrolling is contained with retained offsets;
  edge-reveal drag handles animate; archives are virtualized with global ordering,
  local-calendar buckets and project/model subtitles. Cached projections avoid full
  workspace work on unchanged streams and age time labels while idle. Draft/unread
  dots and rich session/SSH project tooltips are wired to real state. Model menus are
  bounded, scrollable and provider-grouped; effort names are readable. Working/Stopping
  uses Vue's assistant/compaction logic without adding synthetic transcript entries.
- Shared `tau-observability` preserves existing Tauri preferences, telemetry and local
  feedback contracts. Native `iddqd` outside fields toggles the same persisted gate;
  the retained reporter and enabled-only palette actions use real worker acknowledgements.
  Accepted writes remain tracked through disable/hide; no timeout invents a failed save.
- At that historical checkpoint, credential-free **and model-enabled** native acceptance passed admin unlock,
  actual checkbox/report submission, retained editor dismissal/reopen, persisted disable,
  and the new completion-gated last-Cmd-W quit with all Pi child exit acknowledgements.
  The model run also passed assistant/tools/Stop and rename/archive/restore. This supersedes
  the earlier production close/reopen sequence, not its historical failures or generic
  host coverage. No activation repair, arbitrary sleeps or widened gates were used.
- Integrated regressions caught archive width/anchor issues, a model-revision variable
  shadowing the durability identity flag, and a real report pointer-dismissal focus steal.
  These are fixed. The native checkbox test now exercises toggles instead of requiring an
  explicit false disabled snapshot value. Original failure logs remain preserved.
- Session-switch follow-up: Cmd-P → switch → immediate Cmd-K now retains mounted
  keyboard routing through destination paint; new sessions focus their composer when
  ready. Blank unregistered sessions retire on navigation, or after pending reads prove
  them empty. Captured registration acknowledgements, cleanup fences and retained drafts,
  history, queues and extension work prevent destructive cleanup; Pi files are untouched.
  Empty composer text starts 12px higher with unchanged outer/footer geometry.
  Credential-free and model-enabled native acceptance passed the new sequence and
  cleanup acknowledgement alongside existing workflow/shutdown checks. Negative controls
  and early native focus failures remain recorded; no reactivation or widened gates.
- Focused Vue behaviour audit: stable Pi row identities fix cold-selection hover
  shuffling; per-project hovered membership hides arrivals until pointer exit, matching
  Vue's current contract instead of the earlier append-arrival implementation. Measured
  Root tests cover four cold/four warm selections and lower-project stability.
  Archive/archived-details tooltips and read-only header affordances are complete.
- Empty cleanup no longer enters Retry Save before failure; pending acknowledgements
  still gate exit. Additional audited fixes cover empty keyboard submission versus Stop,
  pending-setting and compaction admission, rename/palette suspension with ordinary
  outside-click commits, fixed Yes/No confirm shortcuts, and queue pending feedback.
  The audit's source contracts and bounds are recorded in `gpui/APP.md`.
- Both native acceptance variants pass real hover-created hidden arrivals, natural
  pointer release, retained drafts, cleanup and guarded shutdown. The model run also
  measures/captures the archive tooltip and retains assistant/tool/session actions.
  Earlier failed tooltip ownership and fixture/bootstrap assumptions remain recorded;
  no arbitrary sleeps, activation repairs or widened interaction gates.
- Transcript/UI correction: delayed cold-history loading, non-prose compaction dividers
  with real active-branch reveal, acknowledged pending-user presentation, compact
  multiline tool headers, quiet busy queues, clipped palette rows/smaller controls and
  shared light/dark scrollbars. Optimistic collapse/reorder and cheap selection writes
  avoid unrelated history rescans. This corrects a concrete summary-projection defect;
  the user's complete malformed-transcript example was not reproduced from user data.
- Both native acceptance variants pass isolated synthetic-history hydration through
  installed Pi's actual `get_messages`/`get_entries`, earlier-layer reveal, reader/source
  preservation and return to the retained draft, plus existing workflows/shutdown.
  Startup metadata is allowed at the active leaf. No production sessions were read and
  no interaction deadline was widened. Invalid divider semantics and early fixture/wire
  assumptions were corrected; their negative evidence remains recorded.
- Eight fresh production Vue/GPUI light/dark pairs and diffs were inspected for activity,
  palette, wheel-scrolled composer and pending transcript/queue. Real painted rows/thumbs
  are asserted. The smaller chevrons and quiet queue deliberately differ from Vue;
  residual text rasterization/rounding and platform scrollbar appearance are disclosed.
  This is focused comparison coverage, not full-app visual or packaged acceptance.
- At the transcript/UI checkpoint, GPUI library: **450 passed, 0 failed, 1 ignored**, plus **13 passed** editor
  integration tests and **62 passed** controller tests (one opt-in Pi canary ignored).
  Formatting, GPUI lint/typecheck, five packaging tests, strict all-target/all-feature
  GPUI Clippy and controller Clippy passed. The actual native application was built
  for both acceptance variants. The broader historical Rust run passed its unit binaries but timed out in
  the existing activity Metal runner; the full suite is not reported green.
- Frontend: **894 tests**, lint, typecheck and build passed. GPUI TypeScript/lint
  passed; its Bun suite had **97 passes / 1 browser-clock failure**. The unchanged
  failing file subsequently passed all four tests. Earlier failure is retained.

Native acceptance is own-process, renderer-measured evidence, not a full hardware,
Dock/Spaces or repeated reliability campaign. Intermediate timing failures remain
preserved; historical credential-free passes do not supersede the current busy-close failure.

## Accepted native foundation

The five logical native commits after `dd6be86` remain valid within their reviewed
scope: modal ownership `a94bc18`, accessible editing `d9c3ee6`, passive diagnostics
`bbe7bce`, native host `7bd44fa`, and owned path chooser `27d96be`. Their historical
validation is not a current application-suite count. Accessible values remain
bounded to 64 KiB, with password/readonly/IME guards. Path selection remains
root/window-owned; stale results are rejected, Save preserves the selected URL,
and logical invalidation does not dismiss an OS panel.

Rendering, Markdown/code/document/Mermaid and reusable controls remain accepted;
do not reopen rendering or expand accessibility diagnostics on the critical path.
Legacy minimize evidence remains unresolved (5 timeouts/200 runs and 4 early Undo
failures); no speculative activation/focus repair or relabelled visual snapshots.

## Remaining delivery gates, in priority order

### 1. Finish essential runtime and workflow parity

- [x] Prove the small local assistant stream/end/idle/durable-file/registration
      slice with the corrected predicate and unchanged deadline.
- [x] Prove real model-issued tool execution and Stop through the native composer;
      retain earlier failures and distinguish the remaining cancellation/queue cases.
- [x] Wire saved-session rename and archive/browse/restore using existing contracts.
- [x] Implement the command palette for supported native workflows, remote import UI,
      scoped model choices and Vue-aligned transcript activity presentation.
- [x] Complete audited Vue command coverage and registry-only project removal with
      acknowledged cleanup and retained failure retries. Tree/branch navigation and
      destructive session-file deletion are not Vue parity gaps in this audit.
- [ ] Prove authenticated real SSH workflows using existing configuration contracts.
- [x] Implement bounded file snapshots, source-faithful paths/notices and retained
      inline multiline extension editing with natural embedded reader ownership.
- [x] Verify actual production descender ink, horizontal clipping, dehover, selection-gap
      retention and slash selection/scrolling at both scales and themes.
- [ ] Complete remaining visual/media/extension coverage and authenticated SSH previews;
      preserve preview security and the disclosed selection-paint differences.
- [x] Restore non-prose compaction boundaries and earlier active-branch layers through
      real Pi history RPC, preserving source identities, reader position and draft.
- [x] Implement bounded replacement probes/materialization, source recovery, explicit
      reconnect and eligible idle eviction with retained drafts/workflow ownership.
- [x] Fence per-session runtime/generation/request and command-incarnation ownership;
      retain predecessor proof and exact storage retries across transport restart.
      Never replay uncertain prompts or invent a parallel Pi runtime/session format.
- [x] Restore the shared admin gate and issue reporter behind `iddqd`, with bounded,
      content-free, best-effort diagnostics and explicit opt-in report attachments.
      Native command/lifecycle records are catalog-bound; this does not claim every
      Vue action milestone has a native equivalent.

### 2. Produce an actual installable GPUI application

- [x] Package the production `tau` entrypoint as a standalone native preview, not a
      renamed playground or the existing Tauri bundle. Debug bundle, Info.plist,
      architecture/linkage, ad-hoc signature and five packaging regressions passed.
- [ ] Verify release-profile packaging and actual packaged local/SSH workflows.
- [ ] Complete applicable signing, notarization, updater trust and release recovery
      contracts before shipping. GitHub workflow changes must publish separate compatible
      native arm64 artifacts and a native feed, not Tauri payloads. See
      [packaging release requirements](../gpui/PACKAGING.md#github-release-and-updater-requirements).
      Do not advertise those as implemented today.
- [ ] Finish relevant automated native/controller/backend checks and fix concrete
      regressions; record final counts only after validation finishes.

### 3. Local/SSH data-safe acceptance and GO/NO-GO

- [ ] Verify existing projects, sessions, drafts/settings and restart recovery with
      isolated data first, without destructive conversion or concurrent profile writers.
- [ ] In the packaged app, accept local and SSH send/stream/tools/Stop, session
      switching/drafts, restart, close/cancel/last-window quit and owned async cleanup.
- [x] Implement captured lifecycle identities, completed-proof/storage drain, batch
      shutdown receipts and FIFO exit fence before approval; preserve work on cancellation.
- [ ] Resolve current owned-window busy-close acceptance failure in both modes without
      assuming an external cause; packaged/SSH lifecycle acceptance also remains.
- [ ] Make an explicit ship/continue/revert decision in the 9–10 October window,
      with concrete blockers and safe rollback. Existing Tau remains the fallback.

**Next checkpoint: remaining workflow/recovery parity and packaged local/SSH
acceptance**, then release/cutover—not another prototype visual shell.

## Scope and repository safety

No general prototype polish, accessibility expansion, catalogue consolidation or
benchmark infrastructure on the critical path. Use focused regressions and existing
checks, not a new full-OS manual matrix. Report missed gates promptly rather than
implying that UI readiness guarantees runtime or release readiness.

Durable code/tests normally belong in logical commits. This plan/status document
has a separately authorized documentation-commit exception; `gpui/APP.md` is the
durable host usage document. Keep the plan update in a separate documentation commit.
Preserve unrelated work, refs, data and backups. A broad rewrite rollback requires
an explicit decision; never automatically reset/revert or destructively migrate data.

## Maintained contracts

- [Quality](quality.md), [Pi integration](pi.md) and [reproduction](reproduction.md).
- [Development storage](development-storage.md) and [CI/testing](ci-testing.md).
- [Releases](releases.md) and [observability](observability.md).
- [Native host](../gpui/src/native/host.md) and [path chooser](../gpui/src/native/paths.md).
- [Native application](../gpui/APP.md) and [controller limits](../crates/tau-session/README.md).
- [Prototype README](../gpui/README.md): historical scoped evidence; older next-step
  wording does not supersede this plan.
