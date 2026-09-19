# Deterministic reproduction scenarios

## Remote file previews

On macOS, activating a file path from a remote project downloads a private, read-only snapshot over the project's registered SSH connection and opens it in Tau's native **Remote Preview** Quick Look panel. Directories remain copy-only and report **Path Copied**. Other platforms retain copy-only behavior.

One request runs at a time, with a 64 MiB file limit and a 30-second transfer timeout. New clicks do not cancel or replace an in-flight request. Switching sessions or scrolling away does not cancel it either. A completed preview replaces the previous panel; its snapshot stays until the next preview or app exit. Quitting during a transfer or force-quitting can leave a private file in the operating-system temporary directory.

This intentionally supports ordinary, non-interactive SSH connections and quiet remote shells. It does not handle shell banners on stdout, special SSH modes, or helper-process cleanup. Previewing never writes remotely, loads content into Tau's webview, or falls back to the default application.

For an interactive check, preview a text file and an image, copy a directory, and try a missing file. Verify a subsequent preview replaces the panel and quitting Tau removes the displayed snapshot. Rendering support depends on installed Quick Look providers.

Checked-in Pi scenarios run the real Tau app against development-only mocked Tauri IPC and events. The same scenario source and browser adapter are used by Playwright and by the interactive runner; neither is included in production builds.

## Run a scenario

List the available names and their purposes:

```sh
bun run repro -- --list
```

Open one in the Vite development app:

```sh
bun run repro -- saved-session-stale-generation
```

The command validates the name before starting Vite and opens the selected `test-scenario` URL. Stop the server with Ctrl-C. Ordinary `bun run dev` does not select or install a scenario; in a plain browser it installs a generic in-memory sandbox instead. That sandbox starts with the `atlas` and `notes` sample projects and resets on every page load.

The `empty-workspace` scenario opens the real app shell with no projects or sessions, so the first-run experience can be reviewed without changing the workspace used by either the production or development app.

The stale-generation scenario starts in the saved `Main` session. Submit exactly `Explain the fixture` in the composer. It streams `Deterministic reply.` and pauses before delivering the stale output. Inspect the working state, then release its one gate and confirm the visible working state is unchanged.

The `saved-session-command-replacement` scenario makes `/mock 42` available in the real composer. Submitting it pauses after Tau sends its immediate command identity probe. Release `before-command-replacement-identity` to return the replacement identity and an empty initial hydration. Pi then emits its real turn sequence and pauses at `before-replacement-assistant`: the extension-injected `Run phase 42` user row must already be visible and remain stable across session switching. Release that gate to stream assistant/tool activity and settle into exactly one hydrated user row plus the assistant reply. `42 • plan` becomes registered and selected after settlement; the command itself does not enter the transcript.

The `phantom-command-registration` scenario starts from the visible `New Session` action. Submit `/mcp`, then release `before-streaming-command-sync`. The command-created `MCP workflow` session stays ephemeral through the identity response and pauses at `before-assistant-settlement` with visible partial output. Release that gate to emit assistant `message_end`; Tau waits for the following Pi RPC barrier, registers the session, starts a long tool, and pauses at `after-message-end-registration`. The session is durable there but still has no archive action because the agent is working. Release the final gate to settle, after which archive becomes available. The command itself never appears as a transcript message.

The `phantom-first-prompt-registration` scenario holds the pointer over a new session while its ordinary first prompt starts. Its gates expose the optimistic row before temporary identity adoption, after Pi returns an empty preflight transcript, around extension dialog activity, and before and after a prompt-triggered replacement identity hydrates. The assistant `message_end` barrier then registers and selects the durable replacement. The user message and selected sidebar row remain continuously visible through every transition.

The `plan-implement-replacement` scenario submits `/mock-workflow`, completes `docs · RHI-6267 · Plan`, then holds Plan's settlement hydration until the delayed replacement probe is also outstanding. The hydration still lacks Plan's streamed assistant while the probe reports `docs · RHI-6267 · Implement`. At `implement-active`, Implement is current and selected while Plan is already listed above the older sessions. Release the gate, switch through `Backup`, and reopen Plan to verify its durable transcript before returning to the still-live Implement phase.

The `phantom-command-only` scenario submits `/usage` from a new session and emits only an extension notification. `Usage only` remains an unarchivable ephemeral row while its runtime is alive, then disappears when `Main` is selected; Tau never registers it.

The `saved-session-unacknowledged-abort` scenario accepts `Stop this fixture`, streams `Partial reply.`, and pauses at `abort-request-consumed` after Stop sends one abort. Release that gate to await Tau's bounded state probe, then release `before-abort-timeout-probe-response` to report Pi idle and hydrate the preserved partial turn. The abort and prompt themselves are never acknowledged.

The `saved-session-compaction` scenario first pauses at `compaction-started` after Pi's start event, then settles into one permanent compacted-history boundary. Submit the draft `Miss the compaction start` that was preserved while compacting to pause again at `missed-start-reconciled`; this time Tau learns compaction only from `get_state.isCompacting`. In both pauses the transcript shows `Compacting…`, the composer remains editable, and Stop is disabled.

The `saved-session-prompt-admission` scenario pauses while `Confirm this fixture prompt` is optimistic, then after `agent_start` confirms it. Its second prompt, `Reconcile this fixture prompt`, receives a successful preflight and an idle state before hydration proves Pi did not record it. While paused, the composer accepts the next draft but Send remains disabled; releasing the gate removes the absent optimistic row.

The history scenarios open a saved session whose transcript arrives already complete, as one created elsewhere does — by a workflow extension, or on another machine. `saved-session-short-history` holds one turn and cannot scroll, `saved-session-history` fills the viewport, and `saved-session-long-history` is read back through rows that were only ever estimated. Nothing is submitted in any of them: the whole point is the first render.

The `saved-session-extension-prompt` scenario opens a saved session and has its extension ask a question. The prompt renders at the end of the transcript and the composer is gone while it stands; it carries its own short Pi timeout, so the composer comes back a second and a half later without an answer being sent. Nothing is submitted.

The process-failure scenarios expose each transport transition without exposing its raw payload in the product UI. `saved-session-bootstrap-process-exit` pauses before failure, after the bridge error, and after exit; after the final gate, selecting `Main` exercises the product's real reconnect path and completes a clean bootstrap. `saved-session-prompt-process-exit` first bootstraps `Main` and `Backup`; select `Backup`, return to `Main`, submit `Fail this fixture`, then use its three gates to inspect partial-output preservation and failure isolation. `remote-phantom-prompt-process-exit` starts a new remote session, fails its first submitted prompt during bootstrap, and verifies that the draft is restored and SSH retry remains available after both the bridge error and process exit. After retry succeeds, clear the restored draft and select `Main`; the unused remote session row must disappear without leaving a working indicator. `remote-saved-session-process-exit` bootstraps a saved remote session, exits its established bridge, and verifies that only the inline Reconnect action starts a second runtime with the same session path.

## Inspect and control a paused scenario

The browser console exposes `window.__TAU_PI_SCENARIO__` only while a scenario is selected. Its methods are:

```js
const repro = window.__TAU_PI_SCENARIO__;
repro.scenario(); // stable metadata for the selected scenario
repro.gates(); // gate names and reached/released state
repro.timeline(); // ordered requests, outputs, and gate transitions
repro.nativeInvocationCount('register_session'); // current mocked native count
repro.hasRegisteredSession('session-id'); // whether registry adoption occurred
await repro.waitForGate('before-stale-generation-output');
await repro.releaseGate('before-stale-generation-output');
repro.verify(); // completion result plus the current timeline
```

Releasing `before-stale-generation-output` delivers the old-generation delta and ends the minimized race, so `verify()` succeeds immediately. The UI intentionally remains in its unchanged working state: this scenario emits no prompt response or `agent_settled`, which avoids starting replacement-probe timers while a person inspects it. Normal settlement is covered separately by `saved-session-conversation`.

`verify()` intentionally reports an incomplete scenario while required requests, outputs, or gates remain.

## Scenario anatomy

Scenario source lives in `tests/support/pi-scenario/`. Each typed scenario has stable metadata, fixed runtime generations, and one ordered tape of expected requests, scripted outputs, and required gates. Request matchers specify only meaningful fields; generated request and runtime IDs are captured by the engine. Responses correlate to those captures. Required gates stop output until explicitly released, and every transition enters the diagnostic timeline.

`catalogue.ts` is the single browser-scenario catalogue. Add a scenario there once; the interactive command's validation/list and the browser adapter both read it. Native command fixtures remain in `src/dev/pi-scenario-adapter.ts`, outside the Pi protocol engine.

Playwright selects scenarios directly with `?test-scenario=<name>`, so headless tests do not use the interactive wrapper. Full-app scenario tests must import the shared fixture from `tests/e2e/fixtures.ts` to verify scenario completion and fail on browser errors.

## From a failure report to a regression

For a future failure, inspect the journal context first without copying private content into fixtures or diagnostics. Minimize the causal RPC exchange into one typed scenario, reproduce it interactively with `bun run repro`, and add assertions against visible product behavior rather than controller internals. Keep the minimized scenario in the catalogue as the regression once it fails before the fix and passes afterward.

Use the browser fake for real-App orchestration and visible behavior, and the fake stdio executable only for native spawn, JSONL, event-tagging, and process-lifecycle coverage. Use the separate real-Pi canary only to detect drift in the installed Pi's safe read-only contract; it is not a deterministic reproduction runner and does not replace either fake-based layer.

## Real Pi compatibility canary

The scenarios above and the native bridge tests use scripted fakes and run in the deterministic default suites. A separate, explicit canary checks the small read-only RPC surface against an installed Pi executable:

```sh
bun run test:pi-contract
```

Pi must be available on `PATH`, or `TAU_PI_PATH` must point to its executable. The canary reports the Pi version it exercised and fails clearly when Pi is missing or incompatible. It runs Pi in an isolated temporary working/config/session directory with offline mode enabled and tools, extensions, skills, prompt templates, themes, context files, and project approval disabled. The child receives no provider credentials. It sends only `get_available_models`, `get_commands`, `get_state`, `get_available_thinking_levels`, and `get_messages`; it never sends a prompt or invokes a command, model, provider, SSH host, or external service.

The checks cover JSONL/process usability, dynamic response IDs, and only fields Tau consumes. They deliberately do not snapshot catalogue contents, provider names, paths, optional fields, or ordering. This command is not part of `bun run test`, Playwright, Cargo, or any other default suite.
