# Deterministic reproduction scenarios

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

The command validates the name before starting Vite and opens the selected `test-scenario` URL. Stop the server with Ctrl-C. Ordinary `bun run dev` does not select or install a scenario.

The stale-generation scenario starts in the saved `Main` session. Submit exactly `Explain the fixture` in the composer. It streams `Deterministic reply.` and pauses before delivering the stale output. Inspect the working state, then release its one gate and confirm the visible working state is unchanged.

## Inspect and control a paused scenario

The browser console exposes `window.__TAU_PI_SCENARIO__` only while a scenario is selected. Its methods are:

```js
const repro = window.__TAU_PI_SCENARIO__;
repro.scenario(); // stable metadata for the selected scenario
repro.gates(); // gate names and reached/released state
repro.timeline(); // ordered requests, outputs, and gate transitions
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

## Real Pi compatibility canary

The scenarios above and the native bridge tests use scripted fakes and run in the deterministic default suites. A separate, explicit canary checks the small read-only RPC surface against an installed Pi executable:

```sh
bun run test:pi-contract
```

Pi must be available on `PATH`, or `TAU_PI_PATH` must point to its executable. The canary reports the Pi version it exercised and fails clearly when Pi is missing or incompatible. It runs Pi in an isolated temporary working/config/session directory with offline mode enabled and tools, extensions, skills, prompt templates, themes, context files, and project approval disabled. The child receives no provider credentials. It sends only `get_available_models`, `get_commands`, `get_state`, `get_available_thinking_levels`, and `get_messages`; it never sends a prompt or invokes a command, model, provider, SSH host, or external service.

The checks cover JSONL/process usability, dynamic response IDs, and only fields Tau consumes. They deliberately do not snapshot catalogue contents, provider names, paths, optional fields, or ordering. This command is not part of `bun run test`, Playwright, Cargo, or any other default suite.
