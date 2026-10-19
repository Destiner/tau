# Native window commands

Call `native::install_window_commands(cx)` during explicit application setup,
before opening windows or composing menus. It installs Cmd-W and Cmd-M once per
App and selects `QuitMode::Explicit`: closing the last window does not itself
quit the application. Components do not install this policy. A host that needs a
different quit policy must set it explicitly after installation; repeated
installation does not reset later host choices.

Compose `native::window_menu()` alongside the host's application and Edit menus.
It contributes Minimize and Close Window without replacing the menu bar. A host
may instead put `CloseWindow` in its File menu. Global availability validators
leave both commands unavailable when no window is active; focused responders
can still consume the actions before the global handlers.

## Close ownership

Actions capture the active window at dispatch and defer the update until action
dispatch unwinds. They never retarget a different window if activation changes
in between. Missing or removed targets are inert.

`Window::request_close(cx)` evaluates the same synchronous veto as the native
close control. Unlike `remove_window()`, it cannot bypass
`on_window_should_close`. It removes only the target window when permitted.
The callback is cloned before invocation, so a guard can replace itself without
a live RefCell borrow. Recursive requests from inside the guard are vetoed.

The native close callback and programmatic request share one evaluator; a
programmatic request does not send `performClose:` while GPUI holds the Window
borrow, and does not run a second native veto.

For an asynchronous confirmation, the host must return false, own its pending
request and cancellation state, and request close again after approval. Repeated
cancelled requests may invoke the guard again. These helpers do not invent an
application model, pending dialog, persistence operation, or runtime shutdown.

## Application termination is separate

Window close is not application quit. This module installs no Cmd-Q, Quit-menu,
Dock-quit or OS-termination handler. `on_app_quit` is cleanup, not a cancellable
confirmation hook. A safe host must supply its own termination policy and must
not infer protection for Quit from a window-close veto.

Keeping the process alive after the last window also requires the host to own
reopening and any background runtime work. No automatic window reconstruction
or last-window observer is installed here.

## Regression coverage

```sh
cargo test --locked --manifest-path gpui/Cargo.toml --lib native::window
```

Two Root/Input windows exercise keyboard and menu-descriptor dispatch, native
close callback ordering, activation changes, vetoed-close draft/focus retention,
allowed close notifications, last-window policy, no-active-window availability,
installation idempotence, minimize routing, and guard replacement/reentrancy.
TestPlatform reports minimize requests; it does not establish OS minimization.

Real macOS verification remains separate: Cmd-W and the red close control must
both respect a veto, Cmd-M must minimize the active window, and closing the last
window must leave the process alive when the host retains the explicit policy.
No zoom/fullscreen or custom window geometry behavior is introduced.

The opt-in `native-os` runner logs public state transitions and window-scoped
AppKit miniaturization/key/main/close notifications. It checks draft and input
focus after each veto path. Minimize still issues one command and requires
`NSWindow.isMiniaturized` within the original five-second deadline; it never
reactivates or retries a failed window. Notifications and the post-invocation
snapshot help distinguish an animation in progress from no native transition,
but do not by themselves prove entry into `miniaturize:` or identify a timeout's
cause. No platform workaround is implied by these diagnostics.
