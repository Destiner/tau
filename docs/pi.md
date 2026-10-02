# Pi

## Extensions

Tau delegates extension discovery and execution to Pi rather than implementing a separate extension runtime. Tau drives that runtime over Pi's RPC protocol.

This matrix describes Tau's extension compatibility with Pi 0.84.1. Queue controls additionally require Pi 0.84.4 or later for `clear_queue`. "Supported" means Tau either renders the feature or preserves Pi's runtime behavior. It does not mean every TUI-only presentation API has a Tau equivalent.

### Support matrix

- **Global and project extension discovery (supported):** Pi loads trusted extensions, settings, skills, models, authentication, and other resources.
- **Extension slash commands (supported):** Tau discovers them through `get_commands` and invokes them through `prompt`. Pi executes them instead of sending them to the model, so Tau does not show them as user messages.
- **Steering and follow-up messages (supported):** While Pi works, Enter steers at the next eligible boundary and Cmd+Enter (Ctrl+Enter on other platforms) queues a follow-up. Pi owns scheduling; the queue above the composer shows Pi's snapshots, not transcript entries.
- **Custom tools and tool lifecycle events (supported):** Tau renders normal tool start, update, end, and result events.
- **Tool hooks, including blocked calls (supported):** Pi runs extension hooks; blocked tools appear as failed tool results.
- **`select`, `confirm`, `input`, and `editor` (supported):** Tau renders the prompt at the end of the originating session's transcript, hides that session's composer until it resolves, and routes the response back to that runtime. Escape cancels the prompt when one of its controls is focused. Titles and messages render as markdown, with web links and local file paths clickable.
- **Repeated and background-session dialogs (supported):** Each session keeps its own prompt while hidden, so users can switch sessions freely. Stale requests are discarded after timeout, process exit, or generation changes.
- **`notify` (supported):** Tau renders transcript-local notices with their project and session origin, rendered as markdown. Successful compaction clears notices from the replaced transcript; notices raised afterward retain their position among continued output.
- **`setStatus` (deferred):** Tau ignores Pi TUI footer statuses; session activity and Tau errors use Tau's native presentation instead.
- **`setEditorText` / `set_editor_text` (supported):** Tau updates the originating session's draft even when that session is hidden.
- **Session replacement (`newSession`, `switchSession`, `withSession`) (supported):** Tau detects replacement identity, keeps it ephemeral until an assistant `message_end` plus an identity-scoped RPC barrier proves Pi persisted it, retires legacy rows Pi never saved, and does not force UI selection to follow the runtime.
- **Prompt reconciliation after a session replacement (supported):** Pi emits no replacement event, so Tau re-checks session identity when a command resolves, when a dialog is answered, and for a few seconds after a run settles.
- **Persistent custom session entries (preserved):** Pi owns the session file. Tau does not render custom entries in the transcript.
- **Session names (`setSessionName`) (supported):** Pi owns the name. Tau renames through `set_session_name`, treats `session_info_changed` as an invalidation, and reads the name with Pi's current session identity before adopting it. The session file's name remains authoritative over Tau's stored title.
- **Session lifecycle events and extension rebinding (supported):** Pi's runtime tears down and rebinds extensions around replacement.
- **Extension state held across a phase (supported):** An idle runtime is kept warm rather than stopped with its session, so a workflow keeps the session-opening context it can only hold in its own process.
- **Model registry/control and `pi.exec()` (supported):** These run inside Pi's normal services and credential environment.
- **`extension_error` (supported):** Tau shows reviewed generic failure copy in the session-owned fallback dialog; it does not expose raw event details.
- **`setWidget` and `setTitle` (deferred):** Pi may emit these RPC requests, but Tau currently ignores them.
- **`ctx.ui.custom()` and TUI components (deferred):** RPC returns `undefined`; Tau does not emulate Pi's terminal component system.
- **Custom message, entry, and tool renderers (deferred):** Tau uses its standard transcript and tool presentation.
- **Custom footer, header, editor, working indicator, and themes (deferred):** These are TUI presentation APIs.
- **Extension shortcuts, flags, and autocomplete providers (deferred):** Tau has no extension-facing controls for these APIs.
- **Extension installation or management UI (deferred):** Install extensions through Pi's normal global, project, package, or settings mechanisms.
- **Dedicated workflow dashboard (deferred):** Workflow phases appear as ordinary Tau sessions.

### Queue delivery and lifetime

While a saved session is working, Tau sends ordinary `prompt` requests with `streamingBehavior: "steer"` or `"followUp"`. Pi decides whether the message is queued or starts an ordinary run if it became idle. Steering messages are delivered as separate user messages together at Pi's next eligible turn boundary; follow-ups run one at a time after the current work. Shift+Enter inserts a newline. Idle submissions use the ordinary prompt path; extension slash commands are not run immediately from the busy queue path.

Before a session's first queue submission, Tau checks Pi's `get_state` modes and sets steering to `all` and follow-ups to `one-at-a-time` if necessary. Pi persists these mode changes as defaults for the relevant local or remote account; they can affect other Pi processes. Tau does not restore old modes because doing so could race with those processes.

**Clear All** issues Pi's `clear_queue` once for both groups. It does not stop the current run, undo messages Pi already selected for delivery, or re-add remaining text. A rejected command shows an in-place upgrade/retry message rather than simulating removal. **Stop** aborts only current work and never clears either queue. Pending messages survive switching between live, warm sessions, but are not stored by Tau or automatically replayed after a process exit, replacement, reconnect, or app restart. Lost queue work gets session-local feedback. A queued but idle runtime is protected from idle eviction.

### Runtime lifetime

Pi supplies `ExtensionCommandContext` (which can open sessions) only to command handlers and `withSession` callbacks. Tool/event handlers receive `ExtensionContext`, which cannot. Workflows must retain the opening context in the Pi process across phases. Reopening a session file restores transcript, marker entries, and name, **not** this handoff. Tau keeps idle runtimes warm and evicts least-recently-active ones only past a limit. After app exit, SSH loss, or eviction, the workflow must resume through its own command.

A new webview claims the application-wide Pi runtime before workspace/session loading. Replacing a claim fences its commands and waits for managed direct children to exit; cleanup failure blocks startup to prevent dual frontend ownership. Reinitializing the same document retains its warm runtimes. This covers local Pi and OpenSSH transport children, **not** remote Pi or tool descendants.

Orderly runtime stops, including app exit and frontend ownership replacement, close the managed child's stdin first and allow up to one second for it to exit. This lets a local SSH child forward EOF to remote Pi before the transport is killed. After that grace period, cleanup attempts to force-kill and reap remaining children, with a three-second total cleanup wait budget shared across all children in a bulk stop. Cleanup failure blocks runtime replacement or frontend takeover; app-exit cleanup remains best-effort. Cleanup success confirms only the direct child's exit, not a remote exit acknowledgement. This policy does not cover force-quit, crashes, sleep, or network partitions.

Remote Pi runtimes use dedicated SSH connections with 15-second server-alive probes and a count limit of three. Connection sharing is disabled for these runtimes, so an unresponsive peer normally makes OpenSSH exit after approximately 45 seconds without affecting an existing shared SSH master. Short-lived remote inspection commands keep their existing timeout policy.

When an established remote connection exits, Tau keeps the session, draft, and interrupted transcript, clears live activity, and offers a session-local **Reconnect** action. Selecting the session does not reconnect it, and reconnecting never resends a prompt or extension response. A responsive SSH server cannot reveal a Pi process that is itself hung, and reopening restores Pi's persisted history rather than lost in-memory extension continuations.

### Sessions Tau had archived

A workflow resumed by its own command re-attaches to the phase session that is already on disk, which may be a session the user archived in the meantime. Registering that identity is Tau adopting a session Pi handed it, so the row comes back out of the archive: a session Tau is showing has to be one the user can select and return to.

Adoption is limited to the identities Pi hands over — a replacement, the session a prompt just created, and the session an extension command leaves behind. Bootstraps and run-state polls only re-state a session Tau already opened, so a hidden session that keeps working does not put its row back on its own. Until Tau's registry agrees, the unregistered row stays in the sidebar, because it is the only handle on a session Pi is running.

Local registration stores the transcript path in the owning project's Tau registry, even when an extension saves the transcript outside that project's default Pi session directory. Tau lists only registered paths whose on-disk Pi header matches the registered identity. Older registry entries without a path still discover files in the default directory; a missing or mismatched explicit path never falls back to a different file. Pi continues to own the transcript itself.

### Sessions Pi never saved

Pi buffers new sessions until a finalized assistant message. `AgentSession` emits assistant `message_end` immediately before synchronously appending and flushing it to JSONL. Tau sends an identity-scoped RPC request at that event; its response is an ordering barrier after the append. Registration requires the same runtime generation, session id, and path on response. This also proves remote durability (Tau does not inspect remote files) and can make a tool-driven first run reopenable before settlement.

Command-created and replacement identities remain ephemeral before that boundary. User history, assistant start, streaming updates, custom entries, extension UI, notifications, and command-only runs such as `/usage` keep the live transcript usable but do not make the row reopenable. Navigation and idle eviction preserve a submitted extension command's provisional row and runtime until its reply and identity/history reconciliation finish. Failed reconciliation retains the row and retries only the reads when selected again, never the command. An empty same-identity command-only session is removed when left after successful reconciliation. A command that changes identity is a replacement instead: its empty successor remains selectable and warm even if command reconciliation overlaps settlement or another identity read. An empty history response does not prove that the replacement is finished or disposable. Post-settlement hydration remains a fallback when the earlier barrier was missed or failed, and settlement replacement checks still preserve a completed predecessor under its captured identity. Materialization verification makes bounded attempts; exhausting them does not prove that a real replacement will never start. Its provisional row remains selectable and its live runtime joins the ordinary warm-idle cache rather than being stopped immediately. The six-idle-runtime limit still applies: eviction, process loss, and app exit cannot preserve an in-memory workflow continuation. Ordinary unused and reconciled command-only sessions remain disposable; releasing a runtime can discard an identity that never crossed either verified boundary.

Opening one does not fail. Pi answers `--session` for a file it cannot find by starting a fresh session under the path it was given, reporting that path with a new session id, so the row reads back as an empty session wearing a different identity. Registering that identity would file a second session at the same path, and the row would mint one more on every visit.

Tau treats the requested path coming back under another id as Pi reporting that the session was never saved: it archives the row and presents the empty session Pi did open as an unsent session, which leaves no trace unless it is used. A replacement always brings its own path, so it is unaffected.
