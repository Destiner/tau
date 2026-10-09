# Native host assembly

This is an opt-in host API, not a Pi-backed application. Component initialization
and the prototype executables do not install it. Tauri still owns the operational
Pi processes, session files, persistence, settings, updates, and project import.
No session/runtime model, clipboard access or user-data write is introduced here.

## Installation

After component initialization, call `NativeHost::install(cx, "Tau", timeout,
HostCallbacks { ... })` once. It installs App / File / Edit / Window menus,
Cmd-Q, Cmd-comma (Settings), Cmd-N (New Session), and the existing Cmd-W / Cmd-M
helpers. Close appears in File, Minimize in Window; Edit uses the existing focused
Base responders. The host explicitly selects `QuitMode::Explicit`, keeping the
process alive after the last window closes. Reinstallation retains the first
callbacks and does not reset subsequent host policy changes.

`host_menus` is also available as a pure descriptor function. Calling it has no
side effects. A product host can compose extra App commands (About, updates,
Hide, Services) separately; this foundation does not invent their behavior.

Inject:

- `settings`: dispatch to the host's own settings surface, even without an active
  window. This does not issue Pi `/settings` or manufacture settings state.
- `new_session`: dispatch Tau's existing New Session command without creating a
  window or inventing session state. The callback owns project/runtime policy.
- `create_window`: open the real root and return its `AnyWindowHandle`, or an
  error. The factory closure may retain/inject the host's existing entities and
  state. The foundation never constructs a session or root view itself.
- `reveal_window`: reveal an existing window without replacing its root. The
  exported `native::reveal_window(target, cx)` helper uses standard Window
  activation (no application activation), including minimized-window reveal on
  macOS. Inject `Rc::new(|target, cx| { /* handle reveal_window(target, cx) */ })`
  so stale-target failures use the host's existing fallback. Custom restore policy
  remains injectable; this helper does not revive removed windows.
- `window_failed`: present the host's reviewed fallback error; raw factory errors
  are not displayed or logged by the foundation.
- Lifecycle `requested` and `settled` callbacks as described below.

Use `host.open_window(cx)` for initial creation and explicit window creation. It attaches
the close guard before revealing the created window. For a window created by
another host path, explicitly call `host.lifecycle().attach(window, cx)`. Attach
owns/replaces that window's close guard and is idempotent for the same window.
There is no implicit component installation.

Cmd-N and the File menu preserve Tau's **New Session** command. The separate
`NewWindow` action has no default shortcut or menu item: it is a window-factory
primitive for a host that explicitly supports multiple windows. Both are
unavailable while Quit is pending or approved, avoiding creation after a shutdown
snapshot. The eventual Pi-backed shell owns session behavior and window sharing;
these helpers do not establish either policy.

## Confirmation and shutdown

A `Request` has an opaque, process-unique identity and `Intent::Close(handle)` or
`Intent::Quit`. Native close and Cmd-W/menu close share the existing Window veto.
Native termination and Cmd-Q/menu Quit share the Core termination guard.

The synchronous guard always vetoes a new request. The `requested` callback is
deferred until dispatch and Window borrows unwind. Repeated requests for the same
intent coalesce; two windows can have distinct pending close requests. Quit
supersedes pending closes and blocks new close requests. `settled` informs the
host when a request is approved, cancelled, expired, superseded, or its target
was removed, so its confirmation surface can retire the matching presentation.
Callbacks run without coordinator borrows and may resolve synchronously.

The host presents its own existing `NativeDialog` (Quit kind where appropriate),
or another product confirmation surface. Bind its `DialogEvent::Submitted` and
`DialogEvent::Cancelled` to the captured Request; do not resolve whichever request
happens to be current when a late event arrives. The root owns the dialog entity,
event subscriptions, busy/error UI, and focus restoration. `settled` should dismiss
only the matching presentation, without replacing the root or editor entities.
Validate that a weak origin is still mounted before restoring focus; identity
`FocusHandle::contains(&same_handle, window)` alone is not mounted validity.
No synthetic confirmation copy or session counts are embedded in this API.

Call `lifecycle.resolve(request, false, cx)` to cancel. Call it with `true` only
when the host's confirmation **and required pre-exit shutdown/persistence** have
succeeded. The host may await asynchronous work before resolving. Failed work
should surface locally, and either cancel the request or allow retry before the
configured timeout. If host state changes so a decision is no longer valid,
cancel that captured request before requesting a new one. Check
`lifecycle.is_pending(request)` before starting each asynchronous phase and use
`settled` to cancel host-owned pending tasks when a request expires or is superseded.
This read-only check verifies both identity and deadline; it does not itself deliver
settlement. A check is not an approval permit: `resolve` rechecks the deadline.

Every pending request stores the supplied deadline (minimum one millisecond),
using GPUI's executor monotonic clock (its simulated clock in tests). Equality
with the deadline is expired. Expiration vetoes the operation, reports
`Outcome::Expired` once, and rejects a late result even if the foreground timeout
callback has not run. A decision retires its elapsed request; a new request retires
elapsed pending requests. The old timer cannot retire a replacement. Deferred
confirmation callbacks also check the deadline before starting host work.
Configure a product-appropriate bound and handle expiration visibly.
A duplicate, stale, expired, foreign-host, superseded, or target-removed decision
returns `false`. Cancellation/expiration never invokes cleanup or drops windows,
roots, or editors. Approved close retries the same Window guard with a one-use
permit; approved Quit retries the platform routine with a one-use permit.
Approval does not silently retarget the active window. A later Quit takes priority
over an approved close that has not yet reached its deferred dispatch.

`App::on_app_quit` remains final cleanup, not confirmation. GPUI gives these
cleanup futures its existing shutdown timeout; the host must not put an
unbounded confirmation or essential fallible persistence operation there.
Direct `App::shutdown` and `Window::remove_window` intentionally remain teardown
primitives, not user commands. Hosts must not use them to implement Close/Quit.

## Core and macOS plumbing

Core7's `App::new_app` installed `Platform::on_quit` to call `shutdown` directly.
The macOS adapter registered that callback under `applicationWillTerminate:` and
ignored its boolean result. A Window veto could not protect menu/Dock/native
termination, and `on_app_quit` could not cancel it.

`native-host-quit-core.patch` adds `App::on_app_should_quit`, evaluated before
cleanup. It clones the callback before invocation (safe replacement), rejects
recursive evaluations, and rejects native termination while AppCell is borrowed.
`App::on_reopen` makes the existing platform callback explicitly installable from
host assembly with an App context.

`native-host-quit-macos.patch` minimally amends GPUI's **existing** delegate:
`applicationShouldTerminate:` invokes the existing on_quit callback and returns
NSTerminateNow (1) / NSTerminateCancel (0). There is no second cleanup invocation
from WillTerminate, no delegate replacement, and no NSTerminateLater protocol.
The host retries termination after asynchronous approval; all ordinary macOS
terminate entrypoints reach the same delegate method. Force-kill cannot be vetoed.
The same small adapter patch corrects the existing reopen method's Objective-C
signature to return BOOL, matching AppKit's delegate contract.
The opt-in native-host test below exercises the actual AppKit termination path.
Physical keyboard events, Dock interaction, and operational Pi shutdown remain
separate acceptance, not claims made by headless or synthetic-editor tests.

## Reopen

The host installs the platform reopen callback explicitly. It chooses an existing
owned active window, otherwise the first still-owned window, and calls the injected
reveal callback. With no owned windows, it calls the factory and attaches the new
window. Factory failure is reported, leaves no phantom tracked window, and permits
retry. Approved Quit makes reopen inert.

The pinned macOS adapter forwards its reopen callback when AppKit reports no
visible windows. `host.reopen(cx)` is also an explicit entrypoint for a future
shell's activation policy. TestPlatform exercises both reuse and no-window factory
behavior. The opt-in native-host test additionally exercises the own-process
reopen delegate and restoration of an actual minimized window using the standard
reveal helper. It does not manipulate or certify Dock delivery.

## Deterministic verification

```sh
CARGO_BUILD_JOBS=2 caffeinate -i cargo test --locked \
  --manifest-path gpui/Cargo.toml --lib native
```

The incremental Core patch includes only termination/reopen APIs and minimal
TestPlatform/TestApp callback delivery hooks. Tests run real Core action dispatch,
Base Root/Input entities, Window close guards, and the installed platform callback.
They use no AppKit launch, OS picker, clipboard, permissions, Pi/SSH, or user data.

Coverage includes Settings descriptors/shortcut, no-active-window dispatch,
two-window close isolation/coalescing, cancelled/repeated requests, preserved
editor identity/drafts, Quit supersession, delayed stale approval after timeout,
foreign-host decisions, target removal, native callback gating while AppCell is
borrowed, guard replacement, synchronous host cancellation, exactly-once cleanup,
last-window survival, reopen reuse, factory failure/retry, and explicit New Window.

## Opt-in AppKit acceptance

```sh
CARGO_BUILD_JOBS=2 caffeinate -i cargo test --locked \
  --manifest-path gpui/Cargo.toml --features native-os-tests --test native-host
```

`tests/native-os/host.rs` is a separate `harness = false` test, gated by the existing
`native-os-tests` feature. Requires macOS with an unlocked WindowServer. Its launcher
copies its own exact executable into a temporary sparse directory (no target search)
and runs two owned children, preserving both results even when the first fails.
Success requires a successful child exit and exactly one completion marker emitted
by `on_app_quit` after the observed approval. Early native exit is a failure even
with exit status zero. A child watchdog bounds AppKit hangs.

The termination child checks native Settings plus local Cmd-comma, native close
veto/cancel/new approval, explicit last-window survival, and own-delegate reopen.
Both children call actual `NSApplication terminate:` and verify veto preserves
native key/visible/active state, editor entity/draft/focus, and zero cleanup. Repeated
terminate/menu requests, native `NSMenu.performKeyEquivalent:` with an owned
synthetic Cmd-Q NSEvent, local GPUI Cmd-Q, and direct own-delegate
`applicationShouldTerminate:` coalesce. Cancellation permits a distinct new request,
stale approval is rejected, and approval invokes cleanup once before the actual
owned child exits. Decisions are driven by observed callbacks, not elapsed sleeps.
The reveal child waits for its owned `NSWindowDidMiniaturizeNotification` before
reopen, rather than racing activation against a still-running minimize animation.

Only initial application activation and factory-time editor focus are supplied;
there is no activation/focus repair after interactions. No global input, clipboard,
AX client/permissions, Dock manipulation, user data, session, or runtime is involved.
These callbacks model ownership, not a Pi shell or a confirmation UI. Standard
reveal is verified in this owned-window setup, not all Space/fullscreen configurations.

Real Pi-backed shell wiring, runtime shutdown, settings surfaces, persistence,
confirmation presentation/focus restoration, hardware Cmd-Q, and actual Dock
interaction remain separate integration/acceptance work.
