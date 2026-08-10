# Tau workflow extension fixture

This fixture exercises the workflow-critical Pi APIs. It can run as a project-local extension or be installed globally as a local Pi package.

From the Tau repository root, install it with:

```sh
pi install "$PWD/fixtures/workflow-extension/.pi/extensions/tau-compatibility.ts"
```

Pi references the source file in place, so edits take effect after restarting the runtime or running `/reload`. Remove the global installation with:

```sh
pi remove "$PWD/fixtures/workflow-extension/.pi/extensions/tau-compatibility.ts"
```

Alternatively, open this directory as a project in Tau and accept Pi's project-trust prompt if it appears. Pi deduplicates the extension when the same source file is both globally installed and project-local.

The fixture makes one model request during session replacement so Pi can invoke the custom tool. Use a configured model.

## Manual flow

1. Run `/tau-compat-ui`.
   - Two select prompts should appear consecutively in the composer area.
   - Switch to another session and back while a prompt is waiting; the prompt should remain attached to its originating session.
   - Confirm, input, and editor prompts should follow.
   - Notifications should appear in the bottom-right corner.
   - The composer should be prefilled with `/tau-compat-session`.
2. Submit `/tau-compat-session`.
   - Pi should create a persistent session named **Tau compatibility replacement** without removing the source session.
   - The replacement uses `withSession` to ask the model to call `tau_compatibility_dialog`.
   - The custom tool should stream an update and show a confirmation while tool execution is active.
   - After the run, the composer should be prefilled with `/tau-compat-state`.
3. Submit `/tau-compat-state`.
   - A notification should report that the replacement session marker was found.
   - The composer should be prefilled with a prompt containing `tau-compatibility-blocked`.
4. Submit the prefilled prompt.
   - The model should attempt the harmless `echo` command with the `bash` tool.
   - The extension's `tool_call` hook should block it, and Tau should show a failed tool result.

Cancellation is valid for every dialog. The fixture continues through the UI sequence so all dialog methods can still be checked.

## Covered APIs

- `select`, `confirm`, `input`, and `editor`
- Back-to-back dialog requests
- `notify` and `setEditorText`
- Custom tool registration, progress, structured details, abort signal, and termination
- Dialogs during tool execution
- Blocking a `bash` tool call from `tool_call`
- `ctx.newSession()` with `parentSession`, `setup`, and `withSession`
- Replacement-session `sendUserMessage()`
- Persistent custom entries and session names
