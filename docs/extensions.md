# Workflows and extensions

Tau delegates extension discovery and execution to Pi rather than implementing a separate extension runtime. Tau drives that runtime over Pi's RPC protocol.

This matrix describes Tau's compatibility with Pi 0.84.1. "Supported" means Tau either renders the feature or preserves Pi's runtime behavior. It does not mean every TUI-only presentation API has a Tau equivalent.

## Support matrix

| Capability                                                         | Support   | Notes                                                                                                                                                                           |
| ------------------------------------------------------------------ | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Global and project extension discovery                             | Supported | Pi loads trusted extensions, settings, skills, models, authentication, and other resources.                                                                                     |
| Extension slash commands                                           | Supported | Tau discovers them through `get_commands` and invokes them through `prompt`. Pi executes them instead of sending them to the model, so Tau does not show them as user messages. |
| Custom tools and tool lifecycle events                             | Supported | Tau renders normal tool start, update, end, and result events.                                                                                                                  |
| Tool hooks, including blocked calls                                | Supported | Pi runs extension hooks; blocked tools appear as failed tool results.                                                                                                           |
| `select`, `confirm`, `input`, and `editor`                         | Supported | Tau replaces the originating session's composer with a non-modal prompt and routes the response back to that runtime.                                                           |
| Repeated and background-session dialogs                            | Supported | Each session keeps its own prompt while hidden, so users can switch sessions freely. Stale requests are discarded after timeout, process exit, or generation changes.           |
| `notify`                                                           | Supported | Tau shows transient bottom-right notifications with their project and session origin.                                                                                           |
| `setStatus`                                                        | Deferred  | Tau ignores Pi TUI footer statuses; session activity and Tau errors use Tau's native presentation instead.                                                                      |
| `setEditorText` / `set_editor_text`                                | Supported | Tau updates the originating session's draft even when that session is hidden.                                                                                                   |
| Session replacement (`newSession`, `switchSession`, `withSession`) | Supported | Tau detects replacement identity, registers every persistent phase session, refreshes session-scoped state, and does not force UI selection to follow the runtime.              |
| Prompt reconciliation after a session replacement                  | Supported | Pi emits no replacement event, so Tau re-checks session identity when a command resolves, when a dialog is answered, and for a few seconds after a run settles.                 |
| Persistent custom session entries                                  | Preserved | Pi owns the session file. Tau does not render custom entries in the transcript.                                                                                                 |
| Session names (`setSessionName`)                                   | Supported | Pi owns the name. Tau renames through `set_session_name`, adopts every `session_info_changed` Pi reports, and prefers the name in the session file over its own stored title.   |
| Session lifecycle events and extension rebinding                   | Supported | Pi's runtime tears down and rebinds extensions around replacement.                                                                                                              |
| Model registry/control and `pi.exec()`                             | Supported | These run inside Pi's normal services and credential environment.                                                                                                               |
| `extension_error`                                                  | Supported | Tau exposes the error as session status text.                                                                                                                                   |
| `setWidget` and `setTitle`                                         | Deferred  | Pi may emit these RPC requests, but Tau currently ignores them.                                                                                                                 |
| `ctx.ui.custom()` and TUI components                               | Deferred  | RPC returns `undefined`; Tau does not emulate Pi's terminal component system.                                                                                                   |
| Custom message, entry, and tool renderers                          | Deferred  | Tau uses its standard transcript and tool presentation.                                                                                                                         |
| Custom footer, header, editor, working indicator, and themes       | Deferred  | These are TUI presentation APIs.                                                                                                                                                |
| Extension shortcuts, flags, and autocomplete providers             | Deferred  | Tau has no extension-facing controls for these APIs.                                                                                                                            |
| Extension installation or management UI                            | Deferred  | Install extensions through Pi's normal global, project, package, or settings mechanisms.                                                                                        |
| Dedicated workflow dashboard                                       | Deferred  | Workflow phases appear as ordinary Tau sessions.                                                                                                                                |

## Compatibility fixture

The project-local fixture at [`fixtures/workflow-extension`](../fixtures/workflow-extension/README.md) covers the workflow-critical path:

- Every supported dialog and two back-to-back selects
- Bottom-right notifications and editor prefill
- A custom tool that opens a dialog during tool execution
- A `tool_call` hook that blocks a marked `bash` call
- Persistent session replacement through `ctx.newSession()`, `setup`, and `withSession`
- A custom session entry and replacement-session name

Open that directory as a Tau project and follow its manual flow. It intentionally avoids modifying the real project and uses a harmless `echo` command for the blocked-tool check.
