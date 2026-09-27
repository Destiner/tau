# Deterministic reproduction scenarios

Checked-in Pi scenarios run the real Tau app against development-only mocked Tauri IPC and events. Playwright and the interactive runner share the same typed tapes and browser adapter; production bundles exclude both. For test strategy and CI timing, see [CI and test execution](ci-testing.md).

## File previews

Regular local and remote files open in Tau's fullscreen viewer. `.md`, `.markdown`, and extensionless `README` render as Markdown (including supported Mermaid diagrams); `.mdx` is highlighted source, not executable MDX. Other code/text uses Ayu highlighting, common images/PDFs/video/audio use safe webview media elements, and unknown binaries show an unavailable-rendering state. Escape or close returns focus to the originating path.

Every regular file is copied into a private read-only snapshot; only that snapshot is exposed through Tauri's dynamically scoped asset protocol. Remote snapshots use the project's registered SSH connection. Preparation is serialized, limited to 64 MiB per snapshot and 30 seconds over SSH. Text/Markdown reads at most 512 KiB and shows truncation or empty-document feedback. Closing or replacing revokes and removes the snapshot; force-quit may leave a private temporary directory. Preview never writes the original.

Relative Markdown file links resolve against the original document directory in the same local/remote project; web links open externally. Linked files replace the snapshot, and closing returns focus to the original transcript path. Document-relative images are deferred rather than fetched from Tau's webview origin; HTTP(S) and inline data images can render. Untrusted HTML is sanitized. This is a reader, not an editor. Local directories open in the OS; remote directories copy their path and show **Path Copied**. SSH requires ordinary non-interactive connections and quiet stdout (no banners or special modes).

For an interactive check, preview short/long/wide code, image, PDF, Markdown links, local and remote directories; check both scrolling axes, directory labels, Escape/close focus, generic missing-file failure copy, and cleanup.

## Archive fixture

Open `/?fixture=archive` in Vite for a synthetic 2,500-session archive across three projects. **Show Archived Sessions** appends batches of 50 on scroll; reopening resets the window. No real transcripts or production metadata are used. `archive-window.e2e.ts` runs in Chromium/WebKit; `archive-performance.e2e.ts` runs in the isolated Chromium performance phase.

## Run and control

```sh
bun run repro -- --list
bun run repro -- saved-session-stale-generation
```

The runner validates the name, opens the `test-scenario` URL in Vite, and stops with Ctrl-C. Plain `bun run dev` selects no scenario: its generic in-memory sandbox has sample `atlas`/`notes` projects and resets on reload. Playwright navigates directly to `?test-scenario=<name>`.

A selected scenario exposes `window.__TAU_PI_SCENARIO__` in the browser console:

```js
const repro = window.__TAU_PI_SCENARIO__;
repro.scenario(); // metadata
repro.gates(); // reached/released state
repro.timeline(); // ordered requests, outputs, transitions
repro.nativeInvocationCount('register_session');
repro.hasRegisteredSession('session-id');
await repro.waitForGate('before-stale-generation-output');
await repro.releaseGate('before-stale-generation-output');
repro.verify(); // completion result and timeline
```

`verify()` reports incomplete until all required requests, outputs, and gates finish. In `saved-session-stale-generation`, submit exactly `Explain the fixture`, wait for `Deterministic reply.`, then release its gate. The stale delta must not change the working UI. This minimized race intentionally has no prompt response or `agent_settled`; use `saved-session-conversation` for normal settlement.

### Scenario index

Use `--list` for the authoritative catalogue and gate names. The following journeys identify the behavior to inspect:

- `empty-workspace` shows the first-run shell without changing real storage. `archived-sessions-review` scrolls past the first 50 archived rows to **Older archived work**: opening browses read-only; Unarchive restores the project row.
- `saved-session-command-replacement`: submit `/mock 42`; release `before-command-replacement-identity`, then `before-replacement-assistant`. The injected `Run phase 42` row remains visible through switching and hydration, settles exactly once with assistant/tool output, and selects `42 • plan`; the command never enters the transcript.
- `phantom-command-registration`: submit `/mcp` in New Session. Release `before-streaming-command-sync`, `before-assistant-settlement`, and `after-message-end-registration` in order. `MCP workflow` remains ephemeral until assistant `message_end` plus Pi RPC barrier; it is registered while a tool still runs, becomes archivable only after settlement, and its transcript path is outside the default project session directory. The command is not a transcript row.
- `phantom-first-prompt-registration`: hold the pointer over a new session during its first ordinary prompt. Gates expose optimistic admission, empty preflight hydration, extension dialog, replacement, and durable registration; the user row and selected sidebar row must never disappear.
- `plan-implement-replacement`: submit `/mock-workflow`; at `plan-hydration-lagged`, Plan remains named/current until `before-implement-identity` confirms the successor. At `implement-active`, Plan appears above older sessions and Implement is selected; switch through Backup and reopen Plan to check its persisted transcript.
- `phantom-command-only`: `/usage` creates a notification-only ephemeral row, never archived or registered, which disappears when Main is selected.
- `saved-session-unacknowledged-abort`: submit `Stop this fixture`, Stop once, then release `abort-request-consumed` and `before-abort-timeout-probe-response`. The idle probe must preserve the partial reply even without prompt/abort acknowledgements.
- `saved-session-compaction`: inspect `compaction-started`, then submit the preserved draft `Miss the compaction start` and inspect `missed-start-reconciled` (state discovered through `get_state.isCompacting`). Both pauses show `Compacting…`, editable composer and disabled Stop. `saved-session-compaction-notifications` uses `before-successful-compaction`, `compacted-hydration-pending`, and `compacted-notifications-reconciled` to check old feedback removal and unique, in-order new notices.
- `saved-session-prompt-admission`: first prompt becomes confirmed on `agent_start`; the second pauses at `before-admission-acknowledgement` and is removed after idle probe plus hydration prove it absent. Draft input remains available while Send is disabled.
- `saved-session-short-history`, `saved-session-history`, and `saved-session-long-history` test first render of already-complete saved histories (no submission), from no scrolling to estimated offscreen rows. `saved-session-extension-prompt` hides the composer until the extension's short timeout expires, with no answer sent.
- `saved-session-bootstrap-process-exit` tests bridge error, exit, fallback dialog and clean reconnect on selecting Main. `saved-session-prompt-process-exit` checks partial output, coalesced fallback, and isolation from Backup. `remote-phantom-prompt-process-exit` restores the first-prompt draft and offers SSH retry; after retry, clearing the draft and switching to Main removes the unused row. `remote-saved-session-process-exit` offers composer-replacing Reconnect for an established bridge and restarts with the same path only on explicit action. No raw transport payload appears in product UI.
- `saved-session-auto-retry` shows the amber `Retrying…` label beside Stop, reviewed reason tooltip, no raw payload, and stable geometry.
- `saved-session-message-queue`: Enter steers and Cmd+Enter follows up while work is held. The rail reflects Pi snapshots, not premature transcript rows. **Clear All** does not abort the run; **Stop** does not clear the queue. `saved-session-queue-rejection` preserves a newer draft and offers Review Unsent/Restore Draft only when the composer is empty. `saved-session-steering-boundary` delivers three steering rows together, then follow-ups one at a time, each hydrated once; Pi, not Tau, schedules them.

## Authoring a regression

Scenario source is `tests/support/pi-scenario/`: stable metadata, fixed runtime generations, and an ordered tape of expected requests, outputs, and required gates. Match only meaningful request fields; the engine captures generated request/runtime IDs and correlates responses. Each transition enters the timeline. Register new scenarios once in `catalogue.ts`; interactive listing and browser adapter both use it. Native command fixtures live in `src/dev/pi-scenario-adapter.ts`, outside the protocol engine.

Full-app tests import `tests/e2e/fixtures.ts` for completion and browser-error checks. For gate/event-driven tapes, opt into `test.use({ pausedClock: true })` before navigation; otherwise real-time probes can overtake assertions and send unexpected `get_state`. Advance `page.clock` explicitly for timer behavior and between separate prompts so timestamps differ. Extension-prompt expiry and delayed replacement need their own explicit clock policy. Do not silently ignore unexpected requests.

From a failure report, inspect journal context without copying private content to fixtures/diagnostics. Minimize the causal RPC exchange into a typed scenario, reproduce with `bun run repro`, then assert visible behavior, not controller internals. Keep the scenario after it fails before the fix and passes afterward. Use browser fakes for real-app orchestration, fake stdio for native spawn/JSONL/event-tagging/process lifecycle, and the explicit real-Pi canary solely for installed-Pi contract drift.

## Real Pi compatibility canary

```sh
bun run test:pi-contract
```

Pi must be on `PATH` or set by `TAU_PI_PATH`. The explicit canary reports its version and fails if missing/incompatible; it is not in default Bun, Playwright, or Cargo suites. It uses isolated temporary working/config/session directories, offline mode, and disables tools, extensions, skills, prompt templates, themes, context files, and project approval. The child receives no provider credentials. It sends only `get_available_models`, `get_commands`, `get_state`, `get_available_thinking_levels`, and `get_messages`: never prompts, commands, models, providers, SSH hosts, or external services. Checks cover process/JSONL usability, dynamic response IDs, and fields Tau consumes, not catalogue contents, provider names, paths, optional fields, or ordering.
