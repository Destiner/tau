# Native host integration

`edit_menu()` returns a GPUI application-menu fragment using the same Base actions
as native inputs, the composer, and Root's text-selection copy handler. Install it
with `App::set_menus` after component initialization, alongside the host's own
application menus. It does not replace menus implicitly or register global edit
handlers or additional shortcuts.

Cut, Copy, Paste and Select All carry the corresponding AppKit selector metadata.
Undo and Redo retain GPUI's native-menu dispatch because Base editors are not
`NSTextView` instances. Editing continues through the active window and focused
responder; the existing input owns selection, undo history and clipboard policy.
No editor entity is retained by the menu fragment.

Menu availability reads the focused responder's current selection, editability
and undo/redo history. An explicit denial blocks capture and parent/global
fallback, rather than editing a different control. Global validators cannot
hide a nearer enabled responder or enable an action without a handler. Action
lists use the same policy. Copy validation uses selection metadata, never the
source-copy provider or a transcript-wide string projection. An empty source
projection fails closed when Copy is activated.

Paste validation checks editability only: it never reads the clipboard or asks
for permission. Actual Paste retains the existing clipboard and stale-target
policy. Native displayed key equivalents and AppKit menu validation still need
real-window verification.

`install_window_commands` and `window_menu` provide explicit host-owned close
and minimize integration; see [window.md](window.md). Application quit, reopening,
settings, file dialogs, runtime shutdown and persistence remain host-owned. A
quit cleanup callback is not a cancellable confirmation hook.

## Regression coverage

```sh
cargo test --locked --manifest-path gpui/Cargo.toml --lib native
```

Tests obtain actions from the actual descriptors and dispatch them through
`App::dispatch_action` to active test windows under Base Root. Real pointer/key
paths establish editor and prose selection. Coverage includes Unicode editing,
independent histories, dialog focus, source-selection Copy, readonly/disabled
controls, and no-focus/no-window behavior. The clipboard is TestPlatform's
isolated clipboard.

Additional tests deliver marked text through the same `EntityInputHandler`
methods as the OS input bridge. They cover UTF-16 replacement, multi-update
composition, atomic undo/redo, cancellation/unmarking and combining characters.
Tau's editable controls opt into consuming application-delivered Enter/Escape
while text remains marked. Normal Enter immediately after commitment still
works; composer Shift+Enter remains a newline. This guards fallback delivery,
not the platform's candidate-window implementation.

Accessibility regressions activate the actual platform callback and inspect
rendered AccessKit updates. They verify control names, changing Send/Queue/Stop
labels and semantics surviving focus-trap wrappers. These are not full editable
text/value/selection projection or modal background-isolation guarantees.

These tests do not establish AppKit selector behavior, native displayed enabled
states, system-clipboard exchange, IME candidate/dead-key hardware behavior, or
VoiceOver editing. Those require separate real-window verification; roles and
headless input alone are not accessibility acceptance.
