# Reproduction

Use deterministic browser fixtures for UI/protocol regressions, fake stdio for native process behavior, and the explicit real-Pi canary for installed-Pi contract drift. See [CI and test execution](ci-testing.md) for test strategy and [development storage](development-storage.md) for native profiles and browser sandbox behavior.

## Scenarios

Checked-in Pi scenarios run the real Tau app against development-only mocked Tauri IPC/events. Playwright and the interactive runner share typed tapes and a browser adapter; production bundles exclude both.

```sh
bun run repro -- --list
bun run repro -- saved-session-stale-generation
```

The runner validates the name, opens Vite's `?test-scenario=<name>`, and stops with Ctrl-C. Plain `bun run dev` uses a generic in-memory sandbox, not a scenario; it resets on reload. Playwright navigates directly to the scenario URL.

Use `--list` for the authoritative names and descriptions; `tests/support/pi-scenario/catalogue.ts` registers them. Inspect gates through the console API below. Read the selected tape and matching `tests/e2e/pi-scenario-*.e2e.ts` for exact prompts and gate order rather than maintaining a second walkthrough here. Coverage includes first run, archives, session replacement/registration, queues, prompt admission, interruption, compaction/history, extension prompts, retries, and local/remote process failure.

Control the selected scenario from the browser console:

```js
const repro = window.__TAU_PI_SCENARIO__;
repro.scenario();
repro.gates();
repro.timeline();
await repro.waitForGate('before-stale-generation-output');
await repro.releaseGate('before-stale-generation-output');
repro.verify();
```

`timeline()` shows ordered requests, outputs, and transitions; `verify()` stays incomplete until required requests, outputs, and gates finish. `nativeInvocationCount('register_session')` and `hasRegisteredSession('session-id')` inspect native fixture effects.

For `saved-session-stale-generation`, submit exactly `Explain the fixture`, wait for `Deterministic reply.`, then release the gate above. The stale delta must not change the UI. This minimized race deliberately has no prompt response or `agent_settled`; use `saved-session-conversation` for normal settlement.

## Authoring a regression

1. Inspect failure/journal context without copying private content into fixtures or diagnostics. Minimize the causal RPC exchange into a tape under `tests/support/pi-scenario/`.
2. Give it stable metadata, fixed runtime generations, ordered requests/outputs, and required gates. Match only meaningful fields; the engine captures generated request/runtime IDs and correlates responses. Never silently ignore unexpected requests.
3. Register it once in `catalogue.ts`. Native command fixtures belong in `src/dev/pi-scenario-adapter.ts`, outside the protocol engine.
4. Reproduce with `bun run repro`, then add a full-app test importing `tests/e2e/fixtures.ts` for completion and browser-error checks. Assert visible behavior, not controller internals; retain the scenario after proving it fails before the fix and passes afterward.

For gate/event-driven tapes, set `test.use({ pausedClock: true })` before navigation: real-time probes can otherwise overtake assertions and send unexpected `get_state`. Advance `page.clock` for timer behavior and between separate prompts so timestamps differ. Extension expiry and delayed replacement need an explicit clock policy. Use fake stdio—not browser mocks—to test native spawning, JSONL, event tagging, and process cleanup.

## Manual fixtures

- **Archive:** open `/?fixture=archive` in Vite for 2,500 synthetic sessions across three projects. Show Archived Sessions appends batches of 50; reopening resets the window. No real transcripts/metadata are used. Coverage lives in `archive-window.e2e.ts` and the isolated Chromium `archive-performance.e2e.ts`.
- **File previews:** check actual computed token colors in both appearances for short supported source and source above 20,000 characters (including near 512 KiB and the last line), plus short/long/wide code, Markdown links, image/PDF/media, local and remote directories, both scrolling axes, Escape/close focus return, generic missing-file feedback, and snapshot cleanup. Regular files use private read-only snapshots, never modify originals, and must revoke access on close/replacement. Local directories open in the OS; remote directories copy their path. SSH needs non-interactive connections with quiet stdout. Format support and limits live in `src/lib/file-preview.ts` and `src-tauri/src/file_preview.rs`; browser coverage is in `tests/e2e/transcript-preview.e2e.ts` and `markdown-preview.e2e.ts`.

## Real Pi compatibility canary

```sh
bun run test:pi-contract
```

Requires Pi on `PATH` or `TAU_PI_PATH`; reports the version and fails if missing/incompatible. It is excluded from default Bun, Playwright, and Cargo suites. It uses isolated temporary working/config/session directories in offline mode, disables tools and extension/resource loading, and passes no provider credentials.

Only read-only RPCs are sent: `get_available_models`, `get_commands`, `get_state`, `get_available_thinking_levels`, and `get_messages`. No prompts, command execution, model/provider calls, SSH, or external services. Assertions cover JSONL/process usability, dynamic response IDs, and consumed fields—not catalogue contents, provider names, paths, optional fields, or ordering. See `scripts/pi-contract.ts` for the exact isolation and checks.
