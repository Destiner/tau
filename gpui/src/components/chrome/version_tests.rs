use super::*;
use gpui_kit::{TestApp, TestAppWindow, base::Root, test::TestWindowExt};

struct VersionShell {
    sidebar: Entity<Sidebar>,
    editor: Entity<InputState>,
    palette: Entity<InputState>,
    palette_open: bool,
}
impl Render for VersionShell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.sidebar.clone())
            .child(
                div().flex_1().child(
                    div()
                        .id("outside-editor")
                        .test_support()
                        .child(Input::new(&self.editor)),
                ),
            )
            .when(self.palette_open, |d| {
                d.child(
                    div()
                        .id("palette-editor")
                        .test_support()
                        .absolute()
                        .top(px(40.))
                        .right(px(20.))
                        .w(px(200.))
                        .child(Input::new(&self.palette)),
                )
            })
    }
}
fn fixture(
    dark: bool,
) -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Sidebar>,
    Entity<VersionShell>,
) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let mut entities = None;
    let mut window = app.open_window(|w, cx| {
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
                dark,
                cx,
            )
        });
        let shell = cx.new(|cx| VersionShell {
            sidebar: sidebar.clone(),
            editor: cx.new(|cx| InputState::new(w, cx).default_value("Draft café 😀")),
            palette: cx.new(|cx| InputState::new(w, cx).default_value("palette query")),
            palette_open: false,
        });
        entities = Some((sidebar, shell.clone()));
        Root::new(shell, w, cx)
    });
    window.simulate_resize(gpui_kit::size(px(600.), px(400.)));
    window.simulate_activation(true);
    window.draw();
    let (sidebar, shell) = entities.unwrap();
    (app, window, sidebar, shell)
}
fn click(window: &mut TestAppWindow<Root>, id: &str) {
    window.draw();
    let target = window.update(|_, w, _| w.find(id.to_owned()).bounds().center());
    window.simulate_mouse_move(target);
    window.draw();
    window.simulate_mouse_down(target, MouseButton::Left);
    window.draw();
    window.simulate_mouse_up(target, MouseButton::Left);
    window.draw();
}
fn press(window: &mut TestAppWindow<Root>, key: &str) {
    // TestAppWindow::simulate_keystroke only sends key-down. Draw outside
    // Root's entity lease and deliver both halves of Base's activation gesture.
    window.draw();
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    window.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    window.draw();
}

#[test]
fn version_trigger_popup_geometry_escape_and_unsupported_primary_are_honest() {
    for dark in [false, true] {
        let (_app, mut window, sidebar, _) = fixture(dark);
        click(&mut window, "version");
        window.update(|_, w, cx| {
            let s = sidebar.read(cx);
            assert!(s.version_open());
            assert!(s.version_surface_focus.is_focused(w));
            assert_eq!(s.update_title, "Updates unavailable");
            assert_eq!(s.update_message, "Updates are not supported in this build.");
            let trigger = w.find("version").bounds();
            let surface = w.find("version-popover").bounds();
            assert_eq!(surface.size.width, px(250.));
            assert_eq!(s.popover_bounds.get(), Some(surface));
            assert!(surface.bottom() <= trigger.top());
            assert!((f32::from(trigger.top() - surface.bottom()) - 5.).abs() <= 1.);
            // At the default sidebar width, end alignment collides with the
            // viewport margin. Assert the exact clamped result, not an impossible alignment.
            let left = (trigger.right() - px(250.)).max(px(9.));
            assert!((f32::from(surface.left() - left)).abs() <= 1.);
            assert!(surface.right() <= w.viewport_size().width - px(9.));
            assert_eq!(w.find("check-updates").disabled(), Some(true));
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let collected = events.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&sidebar, move |_, _, action: &Action, _| {
                collected.borrow_mut().push(action.clone());
            })
        });
        click(&mut window, "check-updates");
        for key in ["cmd-enter", "ctrl-enter", "cmd-shift-enter"] {
            press(&mut window, key);
        }
        assert!(events.borrow().is_empty());
        window.update(|_, _, cx| assert!(sidebar.read(cx).version_open()));
        press(&mut window, "escape");
        window.update(|_, w, cx| {
            let s = sidebar.read(cx);
            assert!(!s.version_open());
            assert!(s.version_focus.is_focused(w));
        });
        // A wider, hydrated sidebar has room for genuine end alignment.
        window.update(|_, _, cx| sidebar.update(cx, |s, cx| s.set_width(320., cx)));
        click(&mut window, "version");
        window.update(|_, w, _| {
            assert!(
                (f32::from(
                    w.find("version-popover").bounds().right() - w.find("version").bounds().right()
                ))
                .abs()
                    <= 1.
            );
        });
    }
}

#[test]
fn version_keyboard_toggle_outside_dismiss_and_modal_admission() {
    let (_app, mut window, sidebar, shell) = fixture(false);
    // Native Root Tab traversal, followed by a complete button press/release.
    for _ in 0..8 {
        press(&mut window, "tab");
        if window.update(|_, w, cx| sidebar.read(cx).version_focus.is_focused(w)) {
            break;
        }
    }
    window.update(|_, w, cx| assert!(sidebar.read(cx).version_focus.is_focused(w)));
    press(&mut window, "enter");
    window.update(|_, _, cx| assert!(sidebar.read(cx).version_open()));
    click(&mut window, "version");
    window.update(|_, w, cx| {
        assert!(!sidebar.read(cx).version_open());
        assert!(sidebar.read(cx).version_focus.is_focused(w));
    });
    press(&mut window, "space");
    click(&mut window, "outside-editor");
    window.update(|_, w, cx| {
        assert!(!sidebar.read(cx).version_open());
        assert!(
            shell
                .read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
        assert_eq!(
            shell.read(cx).editor.read(cx).value().as_ref(),
            "Draft café 😀"
        );
        assert!(sidebar.read(cx).version_origin.is_none());
        sidebar.update(cx, |s, cx| {
            s.set_version_interactive(false, cx);
            s.open_version_status(w, cx);
            assert!(!s.version_open());
        });
    });
    click(&mut window, "version");
    window.update(|_, w, cx| {
        assert!(!sidebar.read(cx).version_open());
        sidebar.update(cx, |s, cx| {
            s.set_version_interactive(true, cx);
            s.open_version_status(w, cx);
            assert!(s.version_open());
        });
    });
    window.draw();
    press(&mut window, "escape");
    window.update(|_, w, cx| {
        assert!(
            shell
                .read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    window.update(|_, w, cx| w.blur(cx));
    _app.run_until_parked();
}

#[test]
fn version_palette_preservation_denies_reveal_and_outside_dismiss_without_focus_steal() {
    let (_app, mut window, sidebar, shell) = fixture(false);
    click(&mut window, "version");
    window.update(|_, _, cx| {
        sidebar.update(cx, |s, cx| s.preserve_version_under_palette(cx));
        shell.update(cx, |s, cx| {
            s.palette_open = true;
            cx.notify();
        });
    });
    click(&mut window, "palette-editor");
    window.update(|_, w, cx| {
        sidebar.update(cx, |s, cx| {
            s.preserve_version_under_palette(cx);
            s.open_version_status(w, cx);
            assert!(s.version_open());
            assert!(!s.version_interactive);
        });
        assert!(
            shell
                .read(cx)
                .palette
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    // Repeated genuine pointer input into the palette must not dismiss/reveal
    // the underlying popup or hand focus back to its trigger.
    click(&mut window, "palette-editor");
    window.update(|_, w, cx| {
        assert!(sidebar.read(cx).version_open());
        assert!(
            shell
                .read(cx)
                .palette
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
        assert_eq!(
            shell.read(cx).palette.read(cx).value().as_ref(),
            "palette query"
        );
        sidebar.update(cx, |s, cx| s.set_version_interactive(false, cx));
        assert!(!sidebar.read(cx).version_open());
        assert!(
            shell
                .read(cx)
                .palette
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    window.update(|_, w, cx| w.blur(cx));
    _app.run_until_parked();
}
