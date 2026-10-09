use super::*;
use gpui_kit::{MouseMoveEvent, TestApp, TestAppWindow, point, test::TestWindowExt};

// A sibling pane proves flex reflow and catches leaked edge gestures, without
// reaching into Scene's independently-owned composer and transcript wiring.
struct Shell {
    sidebar: Entity<Sidebar>,
    clicks: usize,
    downs: usize,
}
impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, _| s.downs += 1))
            .child(self.sidebar.clone())
            .child(
                div()
                    .id("pane")
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .cursor(gpui_kit::CursorStyle::IBeam)
                    .on_click(cx.listener(|s, _, _, _| s.clicks += 1)),
            )
    }
}
fn props() -> SidebarProps {
    SidebarProps {
        sessions: vec![],
        active: String::new(),
        collapsed: false,
        present: true,
        archived: false,
    }
}
fn fixture(dark: bool) -> (TestApp, TestAppWindow<Shell>) {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|_, cx| Shell {
        sidebar: cx.new(|cx| Sidebar::new(props(), projects(), "1.2.3".into(), dark, cx)),
        clicks: 0,
        downs: 0,
    });
    window.draw();
    (app, window)
}
fn rect(window: &mut TestAppWindow<Shell>, id: &str) -> Bounds<Pixels> {
    window.update(|_, w, _| w.find(id.to_owned()).bounds())
}
fn width(window: &TestAppWindow<Shell>) -> f32 {
    window.read(|s, cx| s.sidebar.read(cx).width())
}
fn moving(window: &TestAppWindow<Shell>) -> bool {
    window.read(|s, cx| s.sidebar.read(cx).resizing())
}
fn highlighted(window: &mut TestAppWindow<Shell>) -> bool {
    window.update(|s, w, cx| {
        let highlighted = s.sidebar.read(cx).handle_highlight(w);
        // This is GPUI's completed-frame opacity observation, not just gesture state.
        assert_eq!(w.find("sidebar-resize-line").visible(), highlighted);
        highlighted
    })
}
fn drag_move(window: &mut TestAppWindow<Shell>, position: Point<Pixels>) {
    window.simulate_event(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    window.draw();
}

#[test]
fn resize_pointer_bounds_rounding_off_handle_release_and_reflow() {
    for dark in [false, true] {
        let (_app, mut w) = fixture(dark);
        let sidebar = rect(&mut w, "sidebar");
        let handle = rect(&mut w, "sidebar-resize-handle");
        assert_eq!(sidebar.size.width, px(260.));
        assert_eq!(handle.size.width, px(9.));
        assert_eq!(handle.left(), sidebar.right() - px(6.));
        assert_eq!(handle.right(), sidebar.right() + px(3.));
        assert_eq!(handle.size.height, sidebar.size.height);
        let line = rect(&mut w, "sidebar-resize-line");
        assert_eq!(line.size.width, px(1.));
        assert_eq!(line.left(), sidebar.right() - px(2.));
        let pane_right = rect(&mut w, "pane").right();
        w.simulate_mouse_down(handle.center(), MouseButton::Left);
        w.draw();
        assert!(moving(&w));
        assert!(highlighted(&mut w));
        let target = point(sidebar.left() + px(337.6), handle.center().y);
        drag_move(&mut w, target);
        assert_eq!(width(&w), 338.);
        assert_eq!(rect(&mut w, "sidebar").size.width, px(338.));
        assert_eq!(rect(&mut w, "pane").left(), sidebar.left() + px(338.));
        assert_eq!(rect(&mut w, "pane").right(), pane_right);
        // No move is required at the release position; release applies its final X.
        w.simulate_mouse_up(
            point(sidebar.left() + px(410.), px(-20.)),
            MouseButton::Left,
        );
        w.draw();
        assert_eq!(width(&w), 410.);
        assert!(!moving(&w));
        assert!(!highlighted(&mut w));
        w.simulate_mouse_move(target);
        w.draw();
        assert_eq!(width(&w), 410.);
        w.update(|s, _, _| {
            assert_eq!(s.clicks, 0);
            assert_eq!(s.downs, 0);
        });
        for (x, expected) in [(-100., 200.), (2000., 480.), (241.5, 242.)] {
            let grip = rect(&mut w, "sidebar-resize-handle").center();
            w.simulate_mouse_down(grip, MouseButton::Left);
            w.draw();
            drag_move(&mut w, point(sidebar.left() + px(x), px(-40.)));
            assert_eq!(width(&w), expected);
            w.simulate_mouse_up(point(sidebar.left() + px(x), px(-40.)), MouseButton::Left);
            w.draw();
            assert!(!moving(&w));
        }
    }
}

#[test]
fn resize_hover_release_suppression_pointer_focus_and_keyboard_limits() {
    let (_app, mut w) = fixture(false);
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    assert!(!highlighted(&mut w));
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(highlighted(&mut w));
    w.simulate_mouse_down(grip, MouseButton::Left);
    w.draw();
    w.simulate_mouse_up(grip, MouseButton::Left);
    w.draw();
    assert!(!highlighted(&mut w));
    assert_eq!(width(&w), 259.);
    w.update(|_, window, _| assert_eq!(window.find("sidebar-resize-handle").focused(), Some(true)));
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(!highlighted(&mut w));
    let away = rect(&mut w, "pane").center();
    w.simulate_mouse_move(away);
    w.draw();
    // Pointer focus alone must not light the handle after leaving it.
    assert!(!highlighted(&mut w));
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(highlighted(&mut w));
    w.simulate_mouse_move(away);
    w.draw();
    assert!(!highlighted(&mut w));
    for (key, expected) in [
        ("right", 269.),
        ("shift-right", 309.),
        ("left", 299.),
        ("shift-left", 259.),
        ("home", 200.),
        ("left", 200.),
        ("end", 480.),
        ("shift-right", 480.),
    ] {
        w.simulate_keystroke(key);
        w.draw();
        assert_eq!(width(&w), expected);
        assert!(highlighted(&mut w));
        assert!(!moving(&w));
    }
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(highlighted(&mut w));
}

#[test]
fn resize_outside_release_move_away_and_reenter_restores_hover() {
    let (_app, mut w) = fixture(false);
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(highlighted(&mut w));
    w.simulate_mouse_down(grip, MouseButton::Left);
    w.draw();
    let outside = point(px(340.), px(-20.));
    drag_move(&mut w, outside);
    assert!(highlighted(&mut w));
    assert!(!rect(&mut w, "sidebar-resize-handle").contains(&outside));
    w.simulate_mouse_up(outside, MouseButton::Left);
    w.draw();
    assert!(!moving(&w));
    assert!(!highlighted(&mut w));
    let away = rect(&mut w, "pane").center();
    w.simulate_mouse_move(away);
    w.draw();
    assert!(!highlighted(&mut w));
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    w.simulate_mouse_move(grip);
    w.draw();
    assert!(highlighted(&mut w));
    assert_eq!(width(&w), 340.);
}

#[test]
fn resize_lost_button_and_focus_cleanup_preserve_instance_width() {
    let (_app, mut w) = fixture(false);
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    w.simulate_mouse_down(grip, MouseButton::Left);
    w.draw();
    drag_move(&mut w, point(px(350.), grip.y));
    assert_eq!(width(&w), 350.);
    w.simulate_mouse_move(point(px(390.), grip.y));
    w.draw();
    assert!(!moving(&w));
    assert_eq!(width(&w), 350.);
    let grip = rect(&mut w, "sidebar-resize-handle").center();
    w.simulate_mouse_down(grip, MouseButton::Left);
    w.draw();
    assert_eq!(width(&w), 349.);
    w.update(|s, window, cx| s.sidebar.update(cx, |s, cx| s.focus.focus(window, cx)));
    w.draw();
    assert!(!moving(&w));
    w.simulate_keystroke("end");
    w.draw();
    assert_eq!(width(&w), 349.);
    w.update(|s, window, cx| {
        s.sidebar.update(cx, |s, cx| {
            let mut p = props();
            p.archived = true;
            p.active = "another".into();
            s.set_props(p, cx);
            s.set_theme(true, window, cx);
        })
    });
    w.draw();
    assert_eq!(width(&w), 349.);
    assert_eq!(rect(&mut w, "sidebar").size.width, px(349.));
}

#[test]
fn resize_edge_ignores_other_buttons_and_does_not_leak_into_sibling() {
    let (_app, mut w) = fixture(false);
    let grip = rect(&mut w, "sidebar-resize-handle");
    let edge = point(grip.right() - px(0.5), px(12.));
    for button in [MouseButton::Right, MouseButton::Middle] {
        w.simulate_click(edge, button);
        w.draw();
        assert!(!moving(&w));
        assert_eq!(width(&w), 260.);
        w.read(|s, cx| assert!(!s.sidebar.read(cx).context_menu.read(cx).is_open()));
    }
    w.simulate_mouse_down(edge, MouseButton::Left);
    w.draw();
    assert!(moving(&w));
    let away = rect(&mut w, "pane").center();
    drag_move(&mut w, away);
    w.simulate_mouse_up(away, MouseButton::Right);
    w.draw();
    assert!(moving(&w));
    w.simulate_mouse_down(away, MouseButton::Right);
    w.draw();
    assert!(moving(&w));
    w.simulate_mouse_up(away, MouseButton::Left);
    w.draw();
    w.update(|s, _, _| {
        assert_eq!(s.clicks, 0);
        assert_eq!(s.downs, 0);
    });
    // Normal sibling clicks still work once the resize capture has finished.
    w.simulate_click(away, MouseButton::Left);
    w.draw();
    w.update(|s, _, _| {
        assert_eq!(s.clicks, 1);
        assert_eq!(s.downs, 1);
    });
}

#[test]
fn resize_handle_is_tab_reachable_and_keyboard_focus_paints_without_hover() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|w, cx| {
        let sidebar = cx.new(|cx| Sidebar::new(props(), projects(), "1.2.3".into(), false, cx));
        gpui_kit::base::Root::new(sidebar, w, cx)
    });
    let sidebar = window.read(|root, _| root.view().clone().downcast::<Sidebar>().unwrap());
    window.draw();
    let project = window.update(|_, w, _| w.find("project-toggle-test-project").bounds().center());
    window.simulate_click(project, MouseButton::Left);
    window.simulate_mouse_move(point(px(700.), px(500.)));
    window.draw();
    let mut reached = false;
    for _ in 0..12 {
        window.simulate_keystroke("tab");
        window.draw();
        reached = window.update(|_, w, _| w.find("sidebar-resize-handle").focused() == Some(true));
        if reached {
            break;
        }
    }
    assert!(
        reached,
        "resize handle must participate in Root's actual Tab navigation"
    );
    window.update(|_, w, cx| {
        assert!(sidebar.read(cx).handle_highlight(w));
        assert!(w.find("sidebar-resize-line").visible());
    });
    window.simulate_keystroke("shift-right");
    window.draw();
    window.update(|_, w, cx| {
        assert_eq!(sidebar.read(cx).width(), 300.);
        assert_eq!(w.find("sidebar").bounds().size.width, px(300.));
    });
    window.simulate_keystroke("tab");
    window.draw();
    window.simulate_keystroke("end");
    window.draw();
    window.update(|_, w, cx| {
        assert_eq!(sidebar.read(cx).width(), 300.);
        assert!(!sidebar.read(cx).handle_highlight(w));
        assert!(!w.find("sidebar-resize-line").visible());
    });
}

#[test]
fn resize_window_deactivation_cancels_capture_without_changing_width() {
    let mut app = gpui_kit::TestAppContext::single();
    app.update(gpui_kit::init);
    let (sidebar, cx) =
        app.add_window_view(|_, cx| Sidebar::new(props(), projects(), "1.2.3".into(), false, cx));
    cx.update(|w, cx| {
        w.activate_window();
        w.render_frame(cx);
    });
    cx.run_until_parked();
    let grip = cx.update(|w, _| w.find("sidebar-resize-handle").bounds().center());
    cx.simulate_event(MouseDownEvent {
        position: grip,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    cx.update(|w, cx| w.render_frame(cx));
    cx.simulate_event(MouseMoveEvent {
        position: point(px(320.), grip.y),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.update(|w, cx| w.render_frame(cx));
    cx.update(|_, cx| {
        assert!(sidebar.read(cx).resizing());
        assert_eq!(sidebar.read(cx).width(), 320.);
    });
    cx.deactivate_window();
    cx.update(|w, cx| w.render_frame(cx));
    cx.update(|_, cx| {
        assert!(!sidebar.read(cx).resizing());
        assert_eq!(sidebar.read(cx).width(), 320.);
    });
    cx.simulate_event(MouseMoveEvent {
        position: point(px(450.), grip.y),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    cx.update(|w, cx| w.render_frame(cx));
    cx.update(|_, cx| assert_eq!(sidebar.read(cx).width(), 320.));
}

fn projects() -> Vec<ProjectItem> {
    vec![ProjectItem {
        id: "test-project".into(),
        title: "Test project".into(),
        location: "/test/project".into(),
        collapsed: false,
        sessions: vec![],
    }]
}
