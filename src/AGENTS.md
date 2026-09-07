# Frontend

Vue 3 and TypeScript UI for Tau. Root guidance and the quality rubric still apply.

## Structure

- `components/` - Product surfaces; `components/ui/` contains reusable primitives.
- `composables/state.ts` - Reactive workspace, session, controller, and draft state.
- `composables/useTau.ts` - User actions and orchestration exposed to the component tree.
- `lib/pi/` - Pi RPC lifecycle, model scope, transcript hydration, and error presentation.
- `lib/telemetry/` - Content-free tracing, metrics, logging, privacy, and bounded ingestion.
- `lib/admin-mode.ts` / `lib/admin-code.ts` - The switch that gates telemetry and the issue reporter, and the cheat code that flips it.
- `dev/` - Development-only fixtures and the browser Pi scenario adapter.

## Patterns

- Keep state mutations in the existing state/runtime layers; components should render state and dispatch actions.
- Key asynchronous work by controller, runtime generation, and request identity. Drop stale events before they mutate state.
- Preserve per-session isolation, drafts, optimistic prompts, extension dialogs, and warm-runtime workflow context.
- Transcript entries need stable identity across hydration and streaming so virtualization, measured heights, expansion state, and reader position survive rerenders.
- Sanitize rendered content and open links outside the webview. Remote paths remain text; local path handling is session-directory aware.
- Keep fixtures behind `import.meta.env.DEV` and dynamic imports so production bundles do not include them.
- Use existing traced Tauri invocation and privacy helpers for product operations; raw RPC transport is the narrow exception in `lib/pi/runtime.ts`, and `read_admin_mode` is the one untraced command.
- Telemetry records nothing outside admin mode. A test that expects records to reach the native command must enable it first.

## Testing

- Place focused Vitest tests beside the module as `*.test.ts`; run one with `bun x vitest run <path>`.
- Add Playwright coverage for browser-visible behavior, focus, scrolling, or full-app protocol flows.
- Concurrency fixes must prove the stale or cross-session result cannot overwrite current state.
