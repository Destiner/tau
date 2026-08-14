# Tau workflow handoff fixture

This fixture reproduces the way a multi-session Pi workflow hands off from one
phase to the next, and what that handoff depends on.

Pi gives an extension a session-opening context only inside a command handler or
a `withSession` callback. An extension that opens the next phase itself has to
keep that context in the process it was handed, so the handoff exists only in
that process. Reopening the session file restores the transcript but not the
handoff: the phase then completes with nothing left to open its next session,
writes its advanced state, and asks the operator to run the command again.

That is why Tau keeps idle runtimes warm instead of stopping each one as its
session is hidden. This fixture makes the dependency observable without running
a real workflow.

From the Tau repository root, install it with:

```sh
pi install "$PWD/fixtures/workflow-handoff/.pi/extensions/workflow-handoff.ts"
```

Remove it with the matching `pi remove`. Alternatively, open this directory as a
project in Tau and accept Pi's project-trust prompt.

The workflow runs three phases. Each one costs two short model turns, so a small
model is enough. Progress is kept in `.mock-handoff.json` beside this README,
and `/mock-handoff-reset` starts over.

## Handoff survives (expected)

1. Run `/mock-handoff`.
   - A session named **Mock · Plan** opens and answers `Plan ready — reply go to
finish it.`
2. Reply `go`.
   - The model calls `mock_handoff_complete_phase`, the tool result reads `End
this turn so the Implement session can open`, and **Mock · Implement**
     opens on its own once the turn settles.

## Handoff is lost (the failure being guarded against)

1. Run `/mock-handoff` and wait for **Mock · Plan** to answer.
2. Make Tau stop and restart that session's runtime while the phase is waiting:
   switch to other sessions until this one is released, then select it again. A
   notification reads `Mock handoff lost: Plan reopened in a fresh runtime`.
3. Reply `go`.
   - The phase still completes and `.mock-handoff.json` still advances to
     `implement`, but no session opens. The tool result reads `End this turn,
then run /mock-handoff`, and the composer is prefilled with
     `/mock-handoff`.

Step 2 is what Tau's runtime retention prevents for the sessions a user is
moving between. A runtime released because it fell past the retention limit, or
lost with the app or an SSH connection, still ends in the second flow.

## Covered APIs

- `ctx.newSession()` with `parentSession`, `setup`, and `withSession`
- A command context captured for later use, and Pi's staleness guard on it
- `waitForIdle()` and session replacement after a settled turn
- Custom tools, persistent custom entries, session names, and `setEditorText`
