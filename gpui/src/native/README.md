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
policy. The opt-in AppKit test below checks native selectors, key equivalents,
selection/history enabled states, and command delivery without clipboard access.

`install_window_commands` and `window_menu` provide explicit host-owned close
and minimize integration; see [window.md](window.md). [NativeHost](host.md)
explicitly composes application menus, settings callbacks, guarded close/quit
and reopen policy. File dialogs remain host-owned. Runtime shutdown,
persistence, confirmation presentation and the real
Pi-backed root remain injected host responsibilities. A quit cleanup callback
is not a cancellable confirmation hook.

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
labels, disabled states matching actual control editability/activation, and
semantics surviving focus-trap wrappers. Tau fields opt into editor-owned names,
single-line/multiline/password roles and read-only status. Focus targets the
existing editor handle, checks live disabled state, and cannot leave the
currently active Base focus trap. Wrappers suppress duplicate textbox roles.
Disabled/read-only metadata does not itself block events; the owning editor
remains responsible for interaction policy.

Tests cover Focus within an ordinary dialog, rejecting background-editor Focus
until Escape dismisses it, and preserving readonly/disabled editing guards.
Direct Base modal regressions also exercise three nested traps through real
TestPlatform pointer, Tab/Shift-Tab and editor AccessKit Focus delivery. The
innermost focused trap wins independently of registry iteration order; removing
it permits the enclosing layer, and a bounded Tab search cannot finish outside
the trap. Nested Escape respects disabled dismissal, explicit Close remains
available, and lower-layer Cancel cannot close an ancestor behind the top layer.
Two synthetic windows retain independent trap identity as a nonregression check.

These checks use the existing focus-dependent `active_focus_trap` contract, not
a new global modal shield. They do not establish rejection of all background
actions or full modal tree isolation. Hosts still own opening focus, top-layer
metadata, and restoration to a weak, still-mounted origin or active composer;
Base does not introduce another restoration owner. Named editors additionally
provide [bounded plain-text values and SetValue](editor-accessibility.md), using
the existing input handler. Password values are never exposed. Cursor/selection
APIs and VoiceOver certification remain outside this bounded implementation.

The default tests do not establish OS behavior. The following opt-in test opens
and closes its own synthetic window on an unlocked macOS desktop:

```sh
cargo test --locked --manifest-path gpui/Cargo.toml \
  --features native-os-tests --test native-os
```

It inspects real `NSMenuItem` selectors, key equivalents and enabled states;
activates Select All, Undo and Redo through AppKit; and verifies guarded close
requests, native `performClose:`, and actual minimization. It uses a temporary
copy of its executable to avoid HIToolbox scanning Cargo's large build directory.
An unavailable active/key window fails explicitly rather than silently skipping.
Only the test's own window is changed. Copy, Cut and Paste are validated but never
invoked; the system clipboard is neither read nor overwritten.

A second opt-in test queries the actual NSAccessibility provider for its own
synthetic controls and sends native Focus requests:

```sh
cargo test --locked --manifest-path gpui/Cargo.toml \
  --features native-os-tests --test native-accessibility
```

It checks native field names, single/multiline roles, secure-field subroles,
enabled state and focus delivery to the existing editor handles. An actual
queued Focus action serves as a completion barrier for rejected requests; no
fixed observation delay stands in for delivery. It verifies active-trap
containment and rejection through a retained provider object after disabling
its editor. The provider may already reflect the new state in that last case.

These are normal provider calls inside the test's own process, not an external
AX client: no Accessibility permission, VoiceOver setting or other application
is accessed. The observer does not query text values or selections. The pinned
macOS adapter has no dedicated readonly getter, so the readonly editing guard
remains a source/headless contract rather than an independently observed OS
attribute. Premature app termination fails both desktop tests.

The opt-in `native-host` target separately exercises actual AppKit termination
veto/approval, Settings, close, reopen and minimized-window reveal. See
[host.md](host.md) for commands and the exact evidence boundary.

System-clipboard exchange, physical keyboard/menu tracking, IME candidate/dead-key
hardware behavior, and VoiceOver editing remain separate verification gates.
Roles, synthetic input and direct AppKit delegate calls are not acceptance of
those behaviors.
