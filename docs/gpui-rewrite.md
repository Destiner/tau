# GPUI rewrite: current status and delivery plan

Updated 9 October 2026. This plan supersedes the historical prototype checklist.
The requested delivery/decision window is 9–10 October: finish a usable native Tau
or explicitly decide to revert. This is not a guaranteed delivery date.

## Current boundary

- The operational Vue/Tauri application and its Pi integration remain unchanged.
- Native rendering and reusable controls are accepted within their reviewed scope.
- Markdown, code, document previews and Mermaid are accepted; no known new rendering issue
  currently justifies reopening those reviews.
- The synthetic conversation demonstrates composition, not a real GPUI application.
- `gpui/src/main.rs` still launches component/playground or capture scenes.
  `gpui/src/lib.rs` exposes components and native adapters, not production application state.
- `src-tauri/src/main.rs` and `src-tauri/src/lib.rs` remain the operational entrypoint
  and assembly for Pi, storage, SSH, settings, shutdown and updates.
- There is no claimed shipping GPUI bundle or completed runtime migration.

The next work is a **real GPUI production shell with application-owned state and UI
composition**, reusing accepted components. Do not promote synthetic timers, fake projects,
fixture messages or a synthetic protocol into the product architecture.

## Native batch: committed and validated

Five independently buildable logical commits extend checkpoint `dd6be86`:

| Boundary            | Implemented scope                                                              | Final checkpoint |
| ------------------- | ------------------------------------------------------------------------------ | ---------------- |
| Modal ownership     | Innermost focus containment, bounded Tab traversal and topmost Escape handling | `a94bc18`        |
| Accessible editing  | Bounded accessible values and guarded SetValue                                 | `d9c3ee6`        |
| Passive diagnostics | Minimal passive observation of the unresolved minimize reproduction            | `bbe7bce`        |
| Native host         | Menus, guarded Close/Quit, Settings, New Session and reopen                    | `7bd44fa`        |
| Owned path chooser  | Root/window-owned native selection and preserved Save URL                      | `27d96be`        |

Accessible values are bounded to 64 KiB. SetValue is limited to plain inputs and textareas;
password, readonly and IME/composition guards remain enforced. This is not authorization
for broader accessibility expansion before delivery.

The host provides App/File/Edit/Window menus, guarded close and quit, Settings dispatch,
Cmd-N through the application's New Session callback, last-window survival and reopen.
It does not invent settings, sessions, confirmation state or runtime shutdown policy.
The real root must own those callbacks, confirmation presentation and persistence.

`PathChooser` owns asynchronous delivery to the original root/window and rejects stale
workflow results. It does not import projects, read/write files or authorize access.
Native Save preserves the selected URL instead of guessing away filename extensions.
Logical invalidation does not dismiss an OS panel; shell close/quit policy must account
for that existing platform boundary.

### Evidence and residuals

- Final working-tree Rust suite: **671 passed, 0 failed, 1 existing manual benchmark
  ignored**; both Metal runners passed. All five curated commit boundaries built and
  passed library tests without excluded comparison/synthetic application sources.
- Final curated tree passed all-target/all-feature strict Clippy, formatting,
  typecheck and lint. GPUI Bun tests: **98 passed**. Native GPUI and frontend builds,
  plus the existing unsigned Tauri `Tau.app` build, passed.
- All three opt-in native targets (`native-os`, `native-accessibility`, `native-host`)
  passed against the final curated source. Own-process AppKit host checks include
  Quit veto/approval, close, Settings and reopen/reveal. The corrected termination
  hook is `applicationShouldTerminate:`, not `applicationWillTerminate:`.
- Legacy minimize testing remains flaky: **5 timeouts in 200 runs**, alongside
  **4 early Undo failures**. Neither is a clean full-OS acceptance result.
- The passive reproduction still observes AppKit ignoring minimize in some cases.
  Keep that unresolved; do not add speculative activation/focus repairs.
- Own-process tests do not certify hardware shortcuts, Dock delivery, all Spaces/fullscreen
  configurations, production persistence or real Pi shutdown.
- Existing immutable visual-review snapshots predate the latest fixes. Preserve their
  provenance; do not relabel them as evidence for current sources.

**Checkpoint complete.** The final native smoke run passed; it does not erase the
preserved intermittent failures above. This batch supplies reusable native adapters,
not a production GPUI shell or a Pi-backed application.

## Immediate next step: real shell, not another prototype

Build the production GPUI entrypoint and real root/state ownership first. Compose the
accepted sidebar/header/transcript/composer, selectors, overlays and native adapters.
Keep component probes as tests/reference tools, not as the application runtime.

The user selected the real shell as the next task. These are its delivery gates,
not a claim that runtime migration has already been implemented.

### Gate 1 — Real application shell

- [ ] Add a production GPUI entrypoint distinct from component and synthetic scene launchers.
- [ ] Establish application-owned workspace/session/settings state and stable root entities.
- [ ] Reuse accepted controls and rendering rather than duplicate editors or redesign surfaces.
- [ ] Wire menus, New Session, Settings, reopen, confirmation and path selection to real owners.
- [ ] Preserve editor/draft identity across overlays, session switches and cancelled close/quit.
- [ ] Keep Tauri operational while this shell is incomplete; label incomplete flows honestly.

A composed empty shell is progress, not delivery. The same-day next-work checkpoint must
include the first **real Pi vertical slice**, not merely an attractive window.

### Gate 2 — First real Pi vertical slice, then orchestration parity

- [ ] Reuse Rust process, storage and SSH services; strip Tauri-specific handle/event/command
      coupling behind framework-independent boundaries only where migration requires it.
- [ ] Port remaining TypeScript orchestration from `src/composables/state.ts`,
      `src/composables/useTau/` and `src/lib/pi/` into application-owned Rust state/services.
- [ ] Demonstrate a real project/session, prompt submission and live Pi streaming in the shell,
      with tool activity and Stop working end-to-end.
- [ ] Preserve extension requests, queues, model/effort selection and failure/recovery behavior.
- [ ] Retain per-session runtime isolation and generation/request identity checks: stale replies,
      replaced runtimes and cross-session events cannot mutate another session's state.
- [ ] Preserve drafts, optimistic prompts, reading position and workflow context during switching,
      reconnect, failure and idle eviction.
- [ ] Adapt deterministic Pi scenarios for regression coverage; keep the installed-Pi contract
      check an explicit canary rather than a default test dependency.

Pi continues to own extension execution and session files. Do not introduce a parallel
extension runtime, session format or synthetic protocol as a migration shortcut.

### Gate 3 — Persistence and safe lifecycle

- [ ] Load existing projects, sessions, drafts and settings without destructive conversion.
- [ ] Persist real changes and verify restart recovery against isolated test data first.
- [ ] Bind close/quit confirmation to captured request identities and complete essential
      persistence/runtime cleanup before approval; cancellation must preserve current work.
- [ ] Keep late picker, runtime and asynchronous results bound to their original owners.
- [ ] Preserve local/remote preview security and storage/profile boundaries.
- [ ] Retain bounded, content-free, best-effort diagnostics behind the existing admin switch;
      never record prompts, paths, drafts, credentials, transcripts or raw errors.

### Gate 4 — Installable native app and cutover decision

- [ ] Produce an actual installable native application, not a prototype launcher renamed Tau.
- [ ] Build and run the existing relevant automated native checks and orchestration regressions.
- [ ] Smoke-test launch, existing-data loading, real Pi send/stream/tools/Stop, session switching,
      draft retention, restart, close/quit and reopen in the packaged app.
- [ ] Perform basic targeted manual native checks only where needed; do not invent a massive
      full-OS manual matrix as another prerequisite to moving forward.
- [ ] Satisfy applicable release signing, updater trust and packaging contracts before shipping.
- [ ] Make an explicit ship/continue/revert decision with concrete blockers and a safe rollback.

**Next checkpoint:** real GPUI shell, followed by the first real Pi vertical slice.
Do not treat another synthetic composition as satisfying either gate.

## Timebox and scope discipline

No more general prototype polish, accessibility expansion, catalogue consolidation or
benchmark infrastructure on the critical path. Fix concrete product regressions with focused
regressions; accepted rendering is not an invitation to restart visual exploration.
Use existing automated checks and focused evidence rather than growing comparison tooling.

If the real Pi slice or installable application cannot reach the required gates in the
requested delivery/decision window, report that promptly and choose explicitly whether to
continue or revert. Do not imply that UI readiness guarantees runtime or release readiness.

Durable code and necessary tests normally belong in logical commits; migration/review tools
and catalogue/comparison infrastructure remain local unless separately authorized.
The user explicitly authorized **this plan/status document** as an exception,
kept in a separate documentation commit.

A broad rewrite rollback requires an explicit decision. Preserve work refs, unrelated dirty
files, local data and backups; never use a broad reset/revert or destructive data migration
as an automatic cleanup step. Existing operational Tau remains the fallback until cutover.

## Maintained contracts

- [Quality](quality.md): product behavior and regression requirements.
- [Pi integration](pi.md): extension/session ownership and runtime lifetime.
- [Development storage](development-storage.md): isolated profiles and protected data.
- [Releases](releases.md): packaging, signing, updater trust and recovery.
- [CI/testing](ci-testing.md) and [reproduction](reproduction.md): existing validation paths.
- [Observability](observability.md): admin gating and diagnostic privacy limits.
- [Native host contract](../gpui/src/native/host.md): callbacks, lifecycle and evidence boundaries.
- [Native path contract](../gpui/src/native/paths.md): chooser ownership and platform limitations.
- [Native prototype README](../gpui/README.md): implementation entrypoints and scoped historical
  evidence; its older next-step wording does not supersede this plan.
