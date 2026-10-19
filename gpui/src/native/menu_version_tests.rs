use super::*;
use crate::{
    components::chrome::{Sidebar, SidebarProps},
    native::{HostCallbacks, LifecycleCallbacks, NativeHost, host_menus},
};
use gpui_kit::{
    AppContext, Entity, Focusable, IntoElement, MenuItem, Render, TestApp, TestSupportExt,
    base::{Input, Root, input::InputState},
    div,
    prelude::*,
    test::TestWindowExt,
};
use std::time::Duration;

struct Shell {
    sidebar: Entity<Sidebar>,
    input: Entity<InputState>,
}
impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_start()
            .child(self.sidebar.clone())
            .child(
                div()
                    .id("draft-editor")
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.input)),
            )
    }
}
fn install_host(cx: &mut App) {
    NativeHost::install(
        cx,
        "Tau",
        Duration::from_secs(60),
        HostCallbacks {
            lifecycle: LifecycleCallbacks {
                requested: Rc::new(|_, _| {}),
                settled: Rc::new(|_, _, _| {}),
            },
            new_session: Rc::new(|_| {}),
            create_window: Rc::new(|_| Err(anyhow::anyhow!("unused test factory"))),
            reveal_window: Rc::new(|_, _| {}),
            window_failed: Rc::new(|_| {}),
        },
    );
}

#[test]
fn menu_descriptor_is_present_without_settings_and_denied_without_a_surface() {
    let menus = host_menus("Tau");
    assert!(
        matches!(&menus[0].items[0], MenuItem::Action { name, action, .. }
        if name.as_ref() == "Check for Updates…" && action.as_any().is::<CheckForUpdates>())
    );
    assert!(
        !menus
            .iter()
            .flat_map(|menu| &menu.items)
            .any(|item| matches!(item,
        MenuItem::Action { name, .. } if name.contains("Settings")))
    );
    let mut app = TestApp::new();
    app.update(|cx| {
        crate::components::init(cx, false);
        install_host(cx);
        assert!(!cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
    });
}

#[test]
fn native_menu_reaches_mounted_sidebar_from_editor_and_restores_its_unchanged_draft() {
    let mut app = TestApp::new();
    app.update(|cx| {
        crate::components::init(cx, false);
        install_host(cx);
    });
    let mut shell = None;
    let mut window = app.open_window(|w, cx| {
        let entity = cx.new(|cx| Shell {
            sidebar: cx.new(|cx| {
                Sidebar::new(
                    SidebarProps {
                        sessions: vec![],
                        active: String::new(),
                        collapsed: false,
                        present: false,
                        archived: false,
                    },
                    vec![],
                    "0.1.0".into(),
                    false,
                    cx,
                )
            }),
            input: cx.new(|cx| InputState::new(w, cx).default_value("Draft café 😀")),
        });
        shell = Some(entity.clone());
        Root::new(entity, w, cx)
    });
    let shell = shell.unwrap();
    window.simulate_activation(true);
    window.draw();
    app.update(|cx| {
        cx.update_window(window.handle().into(), |_, w, cx| {
            w.click("draft-editor", cx)
        })
        .unwrap();
    });
    window.update(|_, w, cx| assert!(shell.read(cx).input.read(cx).focus_handle(cx).is_focused(w)));
    app.update(|cx| {
        assert!(cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
        cx.dispatch_action(&CheckForUpdates);
    });
    app.run_until_parked();
    window.draw();
    window.update(|_, w, cx| {
        let s = shell.read(cx);
        assert!(s.sidebar.read(cx).version_open());
        assert_eq!(w.find("version-popover").focused(), Some(true));
        assert_eq!(s.input.read(cx).value().as_ref(), "Draft café 😀");
        assert!(!s.input.read(cx).focus_handle(cx).is_focused(w));
    });
    app.update(|cx| {
        cx.update_window(window.handle().into(), |_, w, cx| w.press("escape", cx))
            .unwrap();
    });
    window.update(|_, w, cx| {
        let s = shell.read(cx);
        assert!(!s.sidebar.read(cx).version_open());
        assert!(s.input.read(cx).focus_handle(cx).is_focused(w));
        let sidebar = s.sidebar.clone();
        sidebar.update(cx, |s, cx| s.set_version_interactive(false, cx));
    });
    app.update(|cx| {
        assert!(!cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
    });
    window.update(|_, w, cx| {
        assert!(!shell.read(cx).sidebar.read(cx).version_open());
        w.blur(cx);
    });
}

#[test]
fn native_version_routes_only_to_active_window_and_dropped_destination_fails_closed() {
    let mut app = TestApp::new();
    app.update(|cx| {
        crate::components::init(cx, false);
        install_host(cx);
    });
    let make = |w: &mut Window, cx: &mut Context<Root>| {
        let sidebar = cx.new(|cx| {
            Sidebar::new(
                SidebarProps {
                    sessions: vec![],
                    active: String::new(),
                    collapsed: false,
                    present: false,
                    archived: false,
                },
                vec![],
                "0.1.0".into(),
                false,
                cx,
            )
        });
        Root::new(sidebar, w, cx)
    };
    let mut first = app.open_window(make);
    let mut second = app.open_window(make);
    first.draw();
    second.simulate_activation(true);
    second.draw();
    app.update(|cx| cx.dispatch_action(&CheckForUpdates));
    first.draw();
    second.draw();
    first.update(|_, w, _| {
        assert!(gpui_kit::base::test_support::find(w, &[], &"version-popover".into()).is_none())
    });
    second.update(|_, w, cx| {
        assert!(gpui_kit::base::test_support::find(w, &[], &"version-popover".into()).is_some());
        // Replace the destination with a weak-only owner; releasing it must not
        // dispatch to either the old root or a different active window.
        let detached = cx.new(|cx| {
            Sidebar::new(
                SidebarProps {
                    sessions: vec![],
                    active: String::new(),
                    collapsed: false,
                    present: false,
                    archived: false,
                },
                vec![],
                "0.1.0".into(),
                false,
                cx,
            )
        });
        detached.update(cx, |_, cx| {
            register_version_surface(w, cx, |_| true, Sidebar::open_version_status)
        });
        drop(detached);
    });
    app.update(|cx| {
        assert!(!cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
    });
    first.update(|_, w, cx| w.blur(cx));
    second.update(|_, w, cx| w.blur(cx));
}

#[test]
fn retained_sidebar_remount_registers_new_window_and_retires_old_window_route() {
    struct Empty;
    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }
    let mut app = TestApp::new();
    app.update(|cx| {
        crate::components::init(cx, false);
        install_host(cx);
    });
    let mut retained = None;
    let mut first = app.open_window(|w, cx| {
        let sidebar = cx.new(|cx| {
            Sidebar::new(
                SidebarProps {
                    sessions: vec![],
                    active: String::new(),
                    collapsed: false,
                    present: false,
                    archived: false,
                },
                vec![],
                "0.1.0".into(),
                false,
                cx,
            )
        });
        retained = Some(sidebar.clone());
        Root::new(sidebar, w, cx)
    });
    first.simulate_activation(true);
    first.draw();
    app.update(|cx| assert!(cx.is_action_available(&CheckForUpdates)));
    let sidebar = retained.unwrap();
    first.update(|root, w, cx| {
        let empty = cx.new(|_| Empty);
        *root = Root::new(empty, w, cx);
        cx.notify();
    });
    first.draw();
    let mut second = app.open_window(|w, cx| Root::new(sidebar.clone(), w, cx));
    second.simulate_activation(true);
    second.draw();
    app.update(|cx| {
        assert!(cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
    });
    second.draw();
    second.update(|_, w, cx| {
        assert!(sidebar.read(cx).version_open());
        assert_eq!(w.find("version-popover").focused(), Some(true));
    });
    app.update(|cx| {
        cx.update_window(second.handle().into(), |_, w, cx| w.press("escape", cx))
            .unwrap();
    });
    first.simulate_activation(true);
    app.update(|cx| {
        assert!(!cx.is_action_available(&CheckForUpdates));
        cx.dispatch_action(&CheckForUpdates);
    });
    second.update(|_, _, cx| assert!(!sidebar.read(cx).version_open()));
}
