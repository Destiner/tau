//! Explicit application assembly, independent of the future Pi-backed root.
use super::lifecycle::{Lifecycle, LifecycleCallbacks};
use super::{CloseWindow, MinimizeWindow, edit_menu, install_window_commands};
use gpui_kit::{AnyWindowHandle, App, Global, KeyBinding, Menu, MenuItem, actions};
use std::{rc::Rc, time::Duration};

actions!(native_host, [Quit, Settings, NewSession, NewWindow]);

pub type WindowFactory = dyn Fn(&mut App) -> gpui_kit::Result<AnyWindowHandle>;
pub type WindowRevealer = dyn Fn(AnyWindowHandle, &mut App);

/// Factory failure is reported by the host in its own fallback surface.
/// No raw paths, errors, or fabricated business rules cross this API.
pub struct HostCallbacks {
    pub lifecycle: LifecycleCallbacks,
    pub settings: Rc<dyn Fn(&mut App)>,
    pub new_session: Rc<dyn Fn(&mut App)>,
    pub create_window: Rc<WindowFactory>,
    /// Reveal an existing root. `reveal_window` supplies standard Window activation
    /// (including minimized-window reveal on macOS); inject a custom policy if needed.
    pub reveal_window: Rc<WindowRevealer>,
    pub window_failed: Rc<dyn Fn(&mut App)>,
}

#[derive(Clone)]
pub struct NativeHost {
    lifecycle: Lifecycle,
    create: Rc<WindowFactory>,
    reveal: Rc<WindowRevealer>,
    failed: Rc<dyn Fn(&mut App)>,
}
impl Global for NativeHost {}

/// Standard owned-window reveal without root replacement or application activation.
/// On macOS, Window activation orders the window front and reveals a minimized one.
/// Report a removed/stale target through the caller's existing failure surface.
pub fn reveal_window(target: AnyWindowHandle, cx: &mut App) -> gpui_kit::Result<()> {
    target.update(cx, |_, window, _| window.activate_window())
}

/// Pure menu composition; no handlers or policy are implicitly installed.
pub fn host_menus(app_name: &str) -> Vec<Menu> {
    vec![
        Menu::new(app_name.to_owned()).items([
            MenuItem::action("Settings…", Settings),
            MenuItem::separator(),
            MenuItem::action(format!("Quit {app_name}"), Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Session", NewSession),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        edit_menu(),
        Menu::new("Window").items([MenuItem::action("Minimize", MinimizeWindow)]),
    ]
}

impl NativeHost {
    /// Call after component initialization. Owns menu composition, Cmd-Q/Cmd-,/
    /// Cmd-N (New Session), explicit last-window policy, termination, and reopen.
    /// Repeated installation is inert and retains the original callbacks.
    pub fn install(
        cx: &mut App,
        app_name: &str,
        timeout: Duration,
        callbacks: HostCallbacks,
    ) -> Self {
        if cx.has_global::<Self>() {
            return cx.global::<Self>().clone();
        }
        install_window_commands(cx);
        cx.set_quit_mode(gpui_kit::QuitMode::Explicit);
        let lifecycle = Lifecycle::install(cx, timeout, callbacks.lifecycle);
        let host = Self {
            lifecycle,
            create: callbacks.create_window,
            reveal: callbacks.reveal_window,
            failed: callbacks.window_failed,
        };
        cx.set_global(host.clone());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-,", Settings, None),
            KeyBinding::new("cmd-n", NewSession, None),
        ]);
        cx.on_action_available::<Quit>(|_, cx| !cx.global::<NativeHost>().lifecycle.is_exiting());
        cx.on_action_available::<NewWindow>(|_, cx| {
            cx.global::<NativeHost>().lifecycle.can_open_window()
        });
        cx.on_action_available::<NewSession>(|_, cx| {
            cx.global::<NativeHost>().lifecycle.can_open_window()
        });
        cx.on_action_available::<Settings>(|_, cx| {
            !cx.global::<NativeHost>().lifecycle.is_exiting()
        });
        cx.on_action(|_: &Quit, cx| cx.quit());
        let settings = callbacks.settings;
        cx.on_action(move |_: &Settings, cx| {
            let settings = settings.clone();
            cx.defer(move |cx| settings(cx));
        });
        let new_session = callbacks.new_session;
        cx.on_action(move |_: &NewSession, cx| {
            let new_session = new_session.clone();
            cx.defer(move |cx| {
                if cx.global::<NativeHost>().lifecycle.can_open_window() {
                    new_session(cx);
                }
            });
        });
        let factory = host.clone();
        cx.on_action(move |_: &NewWindow, cx| {
            let host = factory.clone();
            cx.defer(move |cx| {
                host.open_window(cx);
            });
        });
        let reopen = host.clone();
        cx.on_reopen(move |cx| reopen.reopen(cx));
        cx.set_menus(host_menus(app_name));
        host
    }

    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle.clone()
    }

    /// Use for initial creation too: every successfully created window is guarded.
    /// Existing windows may instead be explicitly attached through lifecycle().
    pub fn open_window(&self, cx: &mut App) -> Option<AnyWindowHandle> {
        if !self.lifecycle.can_open_window() {
            return None;
        }
        match (self.create)(cx) {
            Ok(target) => {
                if target
                    .update(cx, |_, window, cx| self.lifecycle.attach(window, cx))
                    .is_err()
                {
                    (self.failed)(cx);
                    return None;
                }
                (self.reveal)(target, cx);
                Some(target)
            }
            Err(_) => {
                (self.failed)(cx);
                None
            }
        }
    }

    /// Reveal an existing owned root, or invoke the factory only with none left.
    /// AppKit currently forwards reopen only when it reports no visible windows;
    /// this explicit API also supports a future shell's own activation entrypoint.
    pub fn reopen(&self, cx: &mut App) {
        if self.lifecycle.is_exiting() {
            return;
        }
        let windows = self.lifecycle.windows();
        let target = cx
            .active_window()
            .filter(|target| windows.contains(target))
            .or_else(|| windows.first().copied());
        if let Some(target) = target {
            (self.reveal)(target, cx);
        } else {
            self.open_window(cx);
        }
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
