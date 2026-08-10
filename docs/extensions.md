# Workflows and extensions

Tau delegates extension discovery and execution to Pi rather than implementing a separate extension runtime. Both the default RPC process and the optional SDK sidecar use Pi's RPC protocol, so they share the same frontend behavior.

This matrix describes Tau's compatibility with Pi 0.84.1. "Supported" means Tau either renders the feature or preserves Pi's runtime behavior. It does not mean every TUI-only presentation API has a Tau equivalent.

## Support matrix

| Capability                                                         | RPC integration | SDK integration | Notes                                                                                                                                                                 |
| ------------------------------------------------------------------ | --------------- | --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Global and project extension discovery                             | Supported       | Supported       | Pi loads trusted extensions, settings, skills, models, authentication, and other resources.                                                                           |
| Extension slash commands                                           | Supported       | Supported       | Tau discovers them through `get_commands` and invokes them through `prompt`.                                                                                          |
| Custom tools and tool lifecycle events                             | Supported       | Supported       | Tau renders normal tool start, update, end, and result events.                                                                                                        |
| Tool hooks, including blocked calls                                | Supported       | Supported       | Pi runs extension hooks; blocked tools appear as failed tool results.                                                                                                 |
| `select`, `confirm`, `input`, and `editor`                         | Supported       | Supported       | Tau replaces the originating session's composer with a non-modal prompt and routes the response back to that runtime.                                                 |
| Repeated and background-session dialogs                            | Supported       | Supported       | Each session keeps its own prompt while hidden, so users can switch sessions freely. Stale requests are discarded after timeout, process exit, or generation changes. |
| `notify`                                                           | Supported       | Supported       | Tau shows transient bottom-right notifications with their project and session origin.                                                                                 |
| `setStatus`                                                        | Deferred        | Deferred        | Tau ignores Pi TUI footer statuses; session activity and Tau errors use Tau's native presentation instead.                                                            |
| `setEditorText` / `set_editor_text`                                | Supported       | Supported       | Tau updates the originating session's draft even when that session is hidden.                                                                                         |
| Session replacement (`newSession`, `switchSession`, `withSession`) | Supported       | Supported       | Tau detects replacement identity, registers every persistent phase session, refreshes session-scoped state, and does not force UI selection to follow the runtime.    |
| Persistent custom session entries                                  | Preserved       | Preserved       | Pi owns the session file. Tau does not render custom entries in the transcript.                                                                                       |
| Session lifecycle events and extension rebinding                   | Supported       | Supported       | Pi's runtime tears down and rebinds extensions around replacement.                                                                                                    |
| Model registry/control and `pi.exec()`                             | Supported       | Supported       | These run inside Pi's normal services and credential environment.                                                                                                     |
| `extension_error`                                                  | Supported       | Supported       | Tau exposes the error as session status text.                                                                                                                         |
| `setWidget` and `setTitle`                                         | Deferred        | Deferred        | Pi may emit these RPC requests, but Tau currently ignores them.                                                                                                       |
| `ctx.ui.custom()` and TUI components                               | Deferred        | Deferred        | RPC returns `undefined`; Tau does not emulate Pi's terminal component system.                                                                                         |
| Custom message, entry, and tool renderers                          | Deferred        | Deferred        | Tau uses its standard transcript and tool presentation.                                                                                                               |
| Custom footer, header, editor, working indicator, and themes       | Deferred        | Deferred        | These are TUI presentation APIs.                                                                                                                                      |
| Extension shortcuts, flags, and autocomplete providers             | Deferred        | Deferred        | Tau has no extension-facing controls for these APIs.                                                                                                                  |
| Extension installation or management UI                            | Deferred        | Deferred        | Install extensions through Pi's normal global, project, package, or settings mechanisms.                                                                              |
| Dedicated workflow dashboard                                       | Deferred        | Deferred        | Workflow phases appear as ordinary Tau sessions.                                                                                                                      |

## Compatibility fixture

The project-local fixture at [`fixtures/workflow-extension`](../fixtures/workflow-extension/README.md) covers the workflow-critical path:

- Every supported dialog and two back-to-back selects
- Bottom-right notifications and editor prefill
- A custom tool that opens a dialog during tool execution
- A `tool_call` hook that blocks a marked `bash` call
- Persistent session replacement through `ctx.newSession()`, `setup`, and `withSession`
- A custom session entry and replacement-session name

Open that directory as a Tau project and follow its manual flow. It intentionally avoids modifying the real project and uses a harmless `echo` command for the blocked-tool check.
