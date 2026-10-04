# Dialog surfaces · candidate 5

Native-only GPUI prototype. No production Vue, Pi runtime, files, network or OS quit operations.

## Run

```sh
cargo run --manifest-path gpui/Cargo.toml --bin dialogs-probe
cargo run --manifest-path gpui/Cargo.toml --bin dialogs-probe -- --list-checks
cargo run --manifest-path gpui/Cargo.toml --bin dialogs-probe -- --capture /tmp/tau-dialogs-light
cargo run --manifest-path gpui/Cargo.toml --bin dialogs-probe -- --capture /tmp/tau-dialogs-dark --dark
cargo clippy --manifest-path gpui/Cargo.toml --bin dialogs-probe -- -D warnings
```

The capture manifest contains intermediate assertions and 2× Metal screenshots; it records actual `Window::focused` state, retained `InputState` / `TextareaState` values, state transitions and popup bounds inside the 900×700 viewport. All controls use real mouse/keyboard events. The failure button is a local explicit completion fixture (not a timer or Pi operation).

## Controls

- **Edit Session** opens a Base `Dialog` with a retained name and notes editor. **Details…** stacks a second Base dialog; Escape dismisses the top layer and restores focus to its parent. **Cancel** and Escape restore trigger focus. **Retry / Save** enters busy state, disables submission and dismissal; **Simulate Failure** ends the synthetic operation with an in-place error. Retry preserves drafts. Closing and reopening preserves both fields.
- **Quit Tau** opens a Base `AlertDialog` with non-dismissible backdrop. Confirm enters busy state; the same fixture button surfaces a local error; Escape is vetoed while busy and returns to the Quit trigger afterward. This does **not** quit the app.
- **Extension Prompt** is deliberately **inline**, not trapped inside a modal. Its answer uses a separate retained Input state. Escape cancels locally; **Cancel Prompt** also dismisses. No Pi prompt dispatch.

## Limits / integration notes

- `gpui_kit::open_window` already wraps the view in Base `Root`. Its Tab / Shift-Tab actions use Base `FocusTrapManager` for dialog containment. Every button has its own focus handle (sharing a dialog container handle between controls collapsed the Tab order). Captures assert exact targets at each forward/reverse transition, including wrapping, the single-control nested topmost layer and initial/returned focus.
- Base `TextareaState` binds Tab / Shift-Tab to `IndentInline` / `OutdentInline` and consumes both before Root can cycle focus. This probe intercepts only those two keys while its edit-dialog notes editor has focus, advances GPUI focus in the requested direction, and stops the editor action. All other fields and buttons use Base Root navigation. This makes the notes field participate in modal Tab order rather than indenting its content; a production integration should decide whether that behavior is appropriate for its editor.
- Base `Dialog` supplies the modal host, backdrop handling and focus-trap registration; Base `AlertDialog` supplies the alert role and non-dismissible backdrop. On synthetic completion the fixture moves focus off the disappearing failure button to a still-present action, so Escape can reach the dialog again. The prototype explicitly restores the still-valid trigger/parent handle on close; it does not test invalidated trigger fallback.
- Headless Metal capture does not validate macOS window ordering, VoiceOver announcements, accessibility tree semantics, IME composition, pointer hit testing outside the sampled interactions, or production integration. Recheck all of those in a native interactive window. In-memory drafts are not persisted across restart.
