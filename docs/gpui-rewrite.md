# GPUI rewrite: current status and delivery plan

Updated 9 October 2026. This supersedes the historical prototype checklist.
The delivery/decision window remains **9–10 October**: finish usable native Tau or
make an explicit continue/revert decision. This is not a guaranteed delivery date.

## Current boundary: first real native application implemented

`gpui/src/bin/tau.rs` now launches the production application in `gpui/src/app.rs`.
`tau` is Cargo's default run target; the older `src/main.rs` playground remains a
reference tool, not the application runtime. The real shell reuses accepted
chrome, transcript, composer, selectors, dialogs and native host adapters. It owns
workspace/session/settings state, retained editors and lifecycle requests; it does
not use synthetic projects, timers or fixture transcripts.

Framework-independent process, storage, profile and SSH services were extracted
into `tau-core` in `af98b6d`, preserving existing Tauri service compatibility.
`tau-session` supplies real Pi JSONL RPC orchestration; the shell (`63cba9c`) routes effects
through those shared services. Local and SSH backends are available, including
existing remote-project configuration. **Real SSH acceptance and new remote import
UI are not implemented/proven.** Pi still owns extensions and session files.

Implemented first-slice behavior includes local project import, existing/new
sessions, model/effort selection, prompt/stream/activity/Stop and queue controls,
extension input/select/confirm and custom output, owned folder
selection, drafts, guarded close/quit, last-window survival and reopen. See
[APP.md](../gpui/APP.md) for launch and profile safety instructions and
[controller limits](../crates/tau-session/README.md) for exact protocol boundaries.

**This is an operational native slice, not whole runtime parity or cutover.**
Settings is intentionally absent: no panel, menu item or Cmd-comma shortcut.
Existing persisted theme choices remain readable; otherwise appearance follows the system.
Tauri remains the operational fallback. A standalone, locally ad-hoc-signed
**Tau Native.app** preview now packages the actual GPUI executable (`4366e27`),
with a separate bundle identity and verified metadata/linkage. It has not replaced
Tauri or been certified for distribution. There is no Developer ID signed/notarized
GPUI release or GPUI updater. See [packaging](../gpui/PACKAGING.md).

## Evidence and limits

- Core extraction: **57 unit + 2 integration tests passed**, one explicit canary
  ignored. Existing Tauri compatibility tests and unsigned Tauri bundle passed;
  that bundle is not GPUI packaging.
- `tau-session`: **27 tests passed**, including rename correlation, rollback and
  name bounds; one opt-in installed-Pi canary ignored in the default run.
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
- GPUI library: **263 passed, 0 failed, 1 ignored**, plus **13 passed** editor
  integration tests; all-target/all-feature strict Clippy and the actual `tau`
  build passed. The broader Rust run passed its unit binaries but timed out in
  the existing activity Metal runner; the full suite is not reported green.
- Frontend: **894 tests**, lint, typecheck and build passed. GPUI TypeScript/lint
  passed; its Bun suite had **97 passes / 1 browser-clock failure**. The unchanged
  failing file subsequently passed all four tests. Earlier failure is retained.

Native acceptance is own-process, renderer-measured evidence, not a full hardware,
Dock/Spaces or repeated reliability campaign. Intermediate timing failures remain
preserved; the final credential-free PASS does not erase them.

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
- [ ] Finish session removal and palette workflows; add new remote import UI and
      real SSH smoke coverage using existing configuration contracts.
- [ ] Complete file/media previews, multiline extension editing and applicable
      extension presentation; preserve local/remote preview security.
- [ ] Finish history/tree/branch/navigation and compaction parity, replacement
      watching/materialization, recovery/reconnect and idle eviction. Preserve
      drafts, optimistic prompts, reading position and workflow context.
- [ ] Preserve per-session runtime/generation/request ownership and stale-result
      fencing. Never auto-replay uncertain prompts or invent a parallel Pi runtime
      or session format. Reuse repository-owned contracts and deterministic fakes.
- [ ] Restore required admin reporting/diagnostics only behind `iddqd`, bounded,
      content-free and best-effort; never log content, paths, credentials or raw errors.

### 2. Produce an actual installable GPUI application

- [x] Package the production `tau` entrypoint as a standalone native preview, not a
      renamed playground or the existing Tauri bundle. Debug bundle, Info.plist,
      architecture/linkage, ad-hoc signature and five packaging regressions passed.
- [ ] Verify release-profile packaging and actual packaged local/SSH workflows.
- [ ] Complete applicable signing, notarization, updater trust and release recovery
      contracts before shipping. Do not advertise those as implemented today.
- [ ] Finish relevant automated native/controller/backend checks and fix concrete
      regressions; record final counts only after validation finishes.

### 3. Local/SSH data-safe acceptance and GO/NO-GO

- [ ] Verify existing projects, sessions, drafts/settings and restart recovery with
      isolated data first, without destructive conversion or concurrent profile writers.
- [ ] In the packaged app, accept local and SSH send/stream/tools/Stop, session
      switching/drafts, restart, close/cancel/quit/reopen and owned async cleanup.
- [ ] Confirm captured lifecycle identities, essential persistence and runtime
      shutdown before exit approval; preserve work on cancellation or failure.
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
