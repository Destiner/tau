pub mod activity;
pub mod admin;
pub mod chrome;
pub mod composer;
pub mod diagram;
pub mod diagram_view;
pub mod dialogs;
pub mod files;
pub mod font;
pub mod markdown;
pub mod menus;
pub mod models;
pub mod palette;
pub mod queue;
pub mod support;
pub mod syntax;
pub mod transcript;

use std::borrow::Cow;

use gpui_kit::{
    App, AssetSource, SharedString,
    base::{Theme, ThemeAppearance},
};

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(support::asset(path))
    }

    fn list(&self, _: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

pub fn init(cx: &mut App, dark: bool) {
    crate::text::init();
    gpui_kit::init(cx);
    bind_native_navigation(cx);
    Theme::global_mut(cx).appearance = if dark {
        ThemeAppearance::Dark
    } else {
        ThemeAppearance::Light
    };
    support::install_scrollbar_theme(dark, cx);
    font::init(cx);
}

#[cfg(test)]
mod native_navigation_tests;
#[cfg(test)]
pub(crate) mod renderer_interaction_tests;

struct NativeNavigation {
    _subscription: gpui_kit::Subscription,
}
impl gpui_kit::Global for NativeNavigation {}

pub fn bind_native_navigation(cx: &mut App) {
    if cx.has_global::<NativeNavigation>() {
        return;
    }
    let subscription = cx.intercept_keystrokes(|event, window, cx| {
        let key = &event.keystroke;
        let modifiers = key.modifiers;
        if key.key != "tab"
            || modifiers.control
            || modifiers.platform
            || modifiers.function
            || (modifiers.alt && !cfg!(target_os = "macos"))
            || !window.is_window_active()
            || window.focused(cx).is_some()
            || window.root::<gpui_kit::base::Root>().flatten().is_none()
        {
            return;
        }

        // With no focus, Root is absent from the key dispatch path. Seed only
        // the active trap, then let Base apply its existing wraparound rules.
        if let Some(trap) = gpui_kit::base::active_focus_trap(window, cx) {
            trap.focus(window, cx);
            let action = cx
                .build_action(
                    if modifiers.shift {
                        "root::TabPrev"
                    } else {
                        "root::Tab"
                    },
                    None,
                )
                .expect("registered Base navigation action");
            window.dispatch_action(action, cx);
        } else if modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        cx.stop_propagation();
    });
    // The App owns the subscription; repeated production/test initialization
    // must not install a second interceptor or duplicate Option-Tab bindings.
    cx.set_global(NativeNavigation {
        _subscription: subscription,
    });

    // macOS Option-Tab also traverses controls; reuse Base's focus-trap-aware actions.
    #[cfg(target_os = "macos")]
    for (key, action) in [("alt-tab", "root::Tab"), ("alt-shift-tab", "root::TabPrev")] {
        let action = cx
            .build_action(action, None)
            .expect("registered Base navigation action");
        let binding = gpui_kit::KeyBinding::load(
            key,
            action,
            Some(
                gpui_kit::KeyBindingContextPredicate::parse("Root")
                    .unwrap()
                    .into(),
            ),
            false,
            None,
            &gpui_kit::DummyKeyboardMapper,
        )
        .expect("valid Option-Tab binding");
        cx.bind_keys([binding]);
    }
}
