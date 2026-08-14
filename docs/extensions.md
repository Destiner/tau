# Workflows and extensions

Tau delegates extension discovery and execution to Pi rather than implementing a separate extension runtime. Tau drives that runtime over Pi's RPC protocol.

This matrix describes Tau's compatibility with Pi 0.84.1. "Supported" means Tau either renders the feature or preserves Pi's runtime behavior. It does not mean every TUI-only presentation API has a Tau equivalent.

## Support matrix

| Capability                                                         | Support   | Notes                                                                                                                                                                                                        |
| ------------------------------------------------------------------ | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Global and project extension discovery                             | Supported | Pi loads trusted extensions, settings, skills, models, authentication, and other resources.                                                                                                                  |
| Extension slash commands                                           | Supported | Tau discovers them through `get_commands` and invokes them through `prompt`. Pi executes them instead of sending them to the model, so Tau does not show them as user messages.                              |
| Custom tools and tool lifecycle events                             | Supported | Tau renders normal tool start, update, end, and result events.                                                                                                                                               |
| Tool hooks, including blocked calls                                | Supported | Pi runs extension hooks; blocked tools appear as failed tool results.                                                                                                                                        |
| `select`, `confirm`, `input`, and `editor`                         | Supported | Tau replaces the originating session's composer with a non-modal prompt and routes the response back to that runtime. Titles and messages render as markdown, with web links and local file paths clickable. |
| Repeated and background-session dialogs                            | Supported | Each session keeps its own prompt while hidden, so users can switch sessions freely. Stale requests are discarded after timeout, process exit, or generation changes.                                        |
| `notify`                                                           | Supported | Tau shows transient bottom-right notifications with their project and session origin, rendered as markdown.                                                                                                  |
| `setStatus`                                                        | Deferred  | Tau ignores Pi TUI footer statuses; session activity and Tau errors use Tau's native presentation instead.                                                                                                   |
| `setEditorText` / `set_editor_text`                                | Supported | Tau updates the originating session's draft even when that session is hidden.                                                                                                                                |
| Session replacement (`newSession`, `switchSession`, `withSession`) | Supported | Tau detects replacement identity, registers the phase sessions Pi reports, retires one Pi never saved, and does not force UI selection to follow the runtime.                                                |
| Prompt reconciliation after a session replacement                  | Supported | Pi emits no replacement event, so Tau re-checks session identity when a command resolves, when a dialog is answered, and for a few seconds after a run settles.                                              |
| Persistent custom session entries                                  | Preserved | Pi owns the session file. Tau does not render custom entries in the transcript.                                                                                                                              |
| Session names (`setSessionName`)                                   | Supported | Pi owns the name. Tau renames through `set_session_name`, adopts every `session_info_changed` Pi reports, and prefers the name in the session file over its own stored title.                                |
| Session lifecycle events and extension rebinding                   | Supported | Pi's runtime tears down and rebinds extensions around replacement.                                                                                                                                           |
| Extension state held across a phase                                | Supported | An idle runtime is kept warm rather than stopped with its session, so a workflow keeps the session-opening context it can only hold in its own process.                                                      |
| Model registry/control and `pi.exec()`                             | Supported | These run inside Pi's normal services and credential environment.                                                                                                                                            |
| `extension_error`                                                  | Supported | Tau exposes the error as session status text.                                                                                                                                                                |
| `setWidget` and `setTitle`                                         | Deferred  | Pi may emit these RPC requests, but Tau currently ignores them.                                                                                                                                              |
| `ctx.ui.custom()` and TUI components                               | Deferred  | RPC returns `undefined`; Tau does not emulate Pi's terminal component system.                                                                                                                                |
| Custom message, entry, and tool renderers                          | Deferred  | Tau uses its standard transcript and tool presentation.                                                                                                                                                      |
| Custom footer, header, editor, working indicator, and themes       | Deferred  | These are TUI presentation APIs.                                                                                                                                                                             |
| Extension shortcuts, flags, and autocomplete providers             | Deferred  | Tau has no extension-facing controls for these APIs.                                                                                                                                                         |
| Extension installation or management UI                            | Deferred  | Install extensions through Pi's normal global, project, package, or settings mechanisms.                                                                                                                     |
| Dedicated workflow dashboard                                       | Deferred  | Workflow phases appear as ordinary Tau sessions.                                                                                                                                                             |

## Runtime lifetime

Pi hands out a session-opening context, `ExtensionCommandContext`, only to a command handler and to a `withSession` callback. Tool and event handlers get the plain `ExtensionContext`, which cannot open anything. A workflow that opens its own next phase therefore has to hold that context from the moment its phase starts until the phase ends, and the context lives in the Pi process and nowhere else.

Stopping a runtime is what makes that state disappear. Reopening the session file restores the transcript, the marker entries, and the name, but not the handoff, so a phase that spans a user turn can complete in a process that has no way to open the session that comes next. Tau therefore keeps idle runtimes warm and releases only the least recently active ones past a limit, which covers the sessions a user moves between while a phase waits on them. A runtime lost with the app, with an SSH connection, or to that limit still leaves the workflow to be resumed by its own command.

## Sessions Pi never saved

Pi buffers a session in memory and writes its file only once the session holds an assistant message, so a phase session cancelled before it answers is reported over RPC and exists nowhere else. Tau registers phase sessions as Pi reports them, because that is what puts a running phase in the sidebar, so such a session can outlive the runtime that held it as a row for a session that was never written. A local project lists the session directory and shows the row only once Pi writes it; a remote project lists what Tau registered, which is where the row survives.

Opening one does not fail. Pi answers `--session` for a file it cannot find by starting a fresh session under the path it was given, reporting that path with a new session id, so the row reads back as an empty session wearing a different identity. Registering that identity would file a second session at the same path, and the row would mint one more on every visit.

Tau treats the requested path coming back under another id as Pi reporting that the session was never saved: it archives the row and presents the empty session Pi did open as an unsent session, which leaves no trace unless it is used. A replacement always brings its own path, so it is unaffected.

## Compatibility fixture

The project-local fixture at [`fixtures/workflow-extension`](../fixtures/workflow-extension/README.md) covers the workflow-critical path:

- Every supported dialog and two back-to-back selects
- Bottom-right notifications and editor prefill
- A custom tool that opens a dialog during tool execution
- A `tool_call` hook that blocks a marked `bash` call
- Persistent session replacement through `ctx.newSession()`, `setup`, and `withSession`
- A custom session entry and replacement-session name

Open that directory as a Tau project and follow its manual flow. It intentionally avoids modifying the real project and uses a harmless `echo` command for the blocked-tool check.

The fixture at [`fixtures/workflow-handoff`](../fixtures/workflow-handoff/README.md) covers the runtime lifetime above. It runs a three-phase workflow whose phases wait on the operator, and reports whether the phase can still open its successor, so the difference between a warm runtime and a restarted one is visible without running a real workflow.
