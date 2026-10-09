//! Explicit host installation; never installed by component initialization.
use gpui_kit::{App, Global, KeyBinding, Menu, MenuItem, QuitMode, actions};

actions!(native_window, [CloseWindow, MinimizeWindow]);

struct Installed;
impl Global for Installed {}

/// Install once per App, before opening windows or composing native menus.
///
/// Closing the last window leaves the application running. This installs no Quit
/// action: application termination needs a separate, cancellable host policy.
/// Requires the `window-close-request.patch` and `native-menu-core.patch` Core APIs.
pub fn install_window_commands(cx: &mut App) {
    if cx.has_global::<Installed>() {
        return;
    }
    cx.set_global(Installed);
    cx.set_quit_mode(QuitMode::Explicit);
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
    ]);
    cx.on_action_available::<CloseWindow>(|_, cx| cx.active_window().is_some());
    cx.on_action_available::<MinimizeWindow>(|_, cx| cx.active_window().is_some());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(target) = cx.active_window() {
            // Global handlers can run while the dispatching Window is borrowed.
            // Capture the target now, but update only after dispatch unwinds.
            cx.defer(move |cx| {
                let _ = target.update(cx, |_, window, cx| window.request_close(cx));
            });
        }
    });
    cx.on_action(|_: &MinimizeWindow, cx| {
        if let Some(target) = cx.active_window() {
            cx.defer(move |cx| {
                let _ = target.update(cx, |_, window, _| window.minimize_window());
            });
        }
    });
}

/// Optional fragment to compose with the host's menus, like `edit_menu`.
/// Does not replace the menu bar or install handlers.
pub fn window_menu() -> Menu {
    Menu::new("Window").items([
        MenuItem::action("Minimize", MinimizeWindow),
        MenuItem::action("Close Window", CloseWindow),
    ])
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
