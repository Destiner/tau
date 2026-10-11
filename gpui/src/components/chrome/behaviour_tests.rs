use super::*;
use gpui_kit::test::TestWindowExt;

fn bounds(window: &Window, id: &str) -> Bounds<Pixels> {
    gpui_kit::base::test_support::find(window, &[], &id.to_owned().into())
        .unwrap_or_else(|| panic!("missing {id}"))
        .bounds()
}
fn missing(window: &Window, id: &str) {
    assert!(
        gpui_kit::base::test_support::find(window, &[], &id.to_owned().into()).is_none(),
        "unexpected {id}"
    );
}
fn project(id: &str, sessions: &[&str]) -> ProjectItem {
    ProjectItem {
        id: id.into(),
        title: id.into(),
        location: "/test".into(),
        collapsed: false,
        sessions: sessions
            .iter()
            .map(|id| SessionItem {
                id: (*id).into(),
                title: (*id).into(),
                time: "now".into(),
                status: None,
                unread: false,
                can_archive: true,
            })
            .collect(),
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
fn hold(window: &mut gpui_kit::TestAppWindow<Sidebar>) {
    let point = window.update(|_, w, _| bounds(w, "sidebar-body").center());
    window.simulate_mouse_move(point);
    window.draw();
    window.update(|s, _, _| assert!(s.held_session_order.is_some()));
}

#[test]
fn chrome_hold_preserves_measured_rows_and_lower_projects_including_held_empty() {
    for dark in [false, true] {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| {
            Sidebar::new(
                props(),
                vec![
                    project("first", &["a", "b"]),
                    project("empty", &[]),
                    project("last", &["c"]),
                ],
                "1".into(),
                dark,
                cx,
            )
        });
        window.draw();
        hold(&mut window);
        let before = window.update(|_, w, _| {
            [
                "session-status-a",
                "session-status-b",
                "project-row-empty",
                "empty-sessions-empty",
                "project-row-last",
                "session-status-c",
            ]
            .map(|id| bounds(w, id))
        });
        window.update(|s, _, cx| {
            s.set_projects(
                vec![
                    project("first", &["new", "b", "a"]),
                    project("empty", &["empty-new"]),
                    project("last", &["c"]),
                ],
                cx,
            )
        });
        window.draw();
        window.update(|_, w, _| {
            let after = [
                "session-status-a",
                "session-status-b",
                "project-row-empty",
                "empty-sessions-empty",
                "project-row-last",
                "session-status-c",
            ]
            .map(|id| bounds(w, id));
            assert_eq!(before, after);
            missing(w, "session-status-new");
            missing(w, "session-status-empty-new");
        });
        // Explicit removal closes a real gap, not a reserved phantom slot.
        window.update(|s, _, cx| {
            s.set_projects(
                vec![
                    project("first", &["new", "b"]),
                    project("empty", &["empty-new"]),
                    project("last", &["c"]),
                ],
                cx,
            )
        });
        window.draw();
        window.update(|_, w, _| {
            missing(w, "session-status-a");
            assert_eq!(bounds(w, "session-status-b").top(), before[0].top());
            assert_eq!(
                bounds(w, "project-row-last").top(),
                before[4].top() - px(29.)
            );
        });
        let footer = window.update(|_, w, _| bounds(w, "sidebar-footer").center());
        window.simulate_mouse_move(footer);
        window.draw();
        window.update(|s, w, _| {
            assert!(s.held_session_order.is_none());
            bounds(w, "session-status-new");
            bounds(w, "session-status-empty-new");
            missing(w, "empty-sessions-empty");
        });
    }
}

#[test]
fn chrome_hold_materialization_and_replacement_use_source_identity_but_captured_id_wins() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|_, cx| {
        let mut s = Sidebar::new(
            props(),
            vec![project("first", &["phantom", "other"])],
            "1".into(),
            false,
            cx,
        );
        s.set_projects_with_identities(
            vec![project("first", &["phantom", "other"])],
            BTreeMap::from([("phantom".into(), "runtime".into())]),
            cx,
        );
        s
    });
    window.draw();
    hold(&mut window);
    let top = window.update(|_, w, _| bounds(w, "session-status-phantom").top());
    for ids in [["other", "saved"], ["saved", "other"]] {
        window.update(|s, _, cx| {
            s.set_projects_with_identities(
                vec![project("first", &ids)],
                BTreeMap::from([("saved".into(), "runtime".into())]),
                cx,
            )
        });
        window.draw();
        window.update(|_, w, _| {
            missing(w, "session-status-phantom");
            assert_eq!(bounds(w, "session-status-saved").top(), top);
            assert!(bounds(w, "session-status-other").top() > top);
        });
    }
    // Recapture the materialized phase, then rebind the same runtime to a replacement.
    window.update(|s, _, cx| {
        s.hold_body_order(false, cx);
        s.hold_body_order(true, cx);
    });
    window.update(|s, _, cx| {
        s.set_projects_with_identities(
            vec![project("first", &["replacement", "other", "saved"])],
            BTreeMap::from([("replacement".into(), "runtime".into())]),
            cx,
        )
    });
    window.draw();
    window.update(|_, w, _| {
        assert_eq!(bounds(w, "session-status-saved").top(), top);
        missing(w, "session-status-replacement");
    });
    window.update(|s, _, cx| {
        s.set_projects_with_identities(
            vec![project("first", &["replacement", "other"])],
            BTreeMap::from([("replacement".into(), "runtime".into())]),
            cx,
        )
    });
    window.draw();
    window.update(|_, w, _| assert_eq!(bounds(w, "session-status-replacement").top(), top));
}

#[test]
fn chrome_hold_scope_excludes_titlebar_footer_and_resets_on_archive_mode_change() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|_, cx| {
        Sidebar::new(
            props(),
            vec![project("first", &["a"])],
            "1".into(),
            false,
            cx,
        )
    });
    window.draw();
    for outside in ["sidebar-blank", "sidebar-footer"] {
        hold(&mut window);
        let point = window.update(|_, w, _| bounds(w, outside).center());
        window.simulate_mouse_move(point);
        window.draw();
        window.update(|s, _, _| assert!(s.held_session_order.is_none()));
    }
    hold(&mut window);
    window.update(|s, _, cx| {
        s.set_props(
            SidebarProps {
                archived: true,
                ..props()
            },
            cx,
        )
    });
    window.draw();
    window.update(|s, _, _| assert!(s.held_session_order.is_none()));
    window.update(|s, _, cx| s.set_props(props(), cx));
    window.draw();
    // Re-entering takes a fresh snapshot; mode changes must not retain old row IDs.
    let outside = window.update(|_, w, _| bounds(w, "sidebar-footer").center());
    window.simulate_mouse_move(outside);
    window.draw();
    window.update(|s, _, cx| s.set_projects(vec![project("first", &["new", "a"])], cx));
    window.draw();
    hold(&mut window);
    window.update(|s, _, cx| s.set_projects(vec![project("first", &["a", "new", "later"])], cx));
    window.draw();
    window.update(|_, w, _| {
        assert!(bounds(w, "session-status-new").top() < bounds(w, "session-status-a").top());
        missing(w, "session-status-later");
    });
}

#[test]
fn chrome_archive_action_has_delayed_measured_tooltip_and_emits_only_archive() {
    for dark in [false, true] {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| {
            Sidebar::new(
                props(),
                vec![project("first", &["a"])],
                "1".into(),
                dark,
                cx,
            )
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = events.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, e: &Action, _| {
                received.borrow_mut().push(e.clone())
            })
        });
        window.draw();
        let point = window.update(|_, w, _| bounds(w, "archive-a").center());
        window.simulate_mouse_move(point);
        window.update(|s, _, _| {
            assert_eq!(s.hint.pending.as_deref(), Some("archive-a"));
            assert_eq!(s.hovered_session(), None);
            assert!(s.held_session_order.is_some());
        });
        app.advance_clock(Duration::from_millis(449));
        window.draw();
        window.update(|s, w, _| {
            assert_eq!(s.tooltip_label(), None);
            missing(w, "chrome-tooltip");
        });
        app.advance_clock(Duration::from_millis(1));
        window.draw();
        window.update(|s, w, _| {
            assert_eq!(s.tooltip_label().as_deref(), Some("Archive Session ⌘⇧A"));
            assert!(bounds(w, "chrome-tooltip").bottom() <= bounds(w, "archive-a").top());
        });
        window.simulate_click(point, MouseButton::Left);
        window.draw();
        assert_eq!(*events.borrow(), vec![Action::Archive("a".into())]);
        let footer = window.update(|_, w, _| bounds(w, "sidebar-footer").center());
        window.simulate_mouse_move(footer);
        window.draw();
        window.update(|s, w, _| {
            assert_eq!(s.tooltip_label(), None);
            missing(w, "chrome-tooltip");
        });
        window.simulate_mouse_move(point);
        app.advance_clock(Duration::from_millis(450));
        window.draw();
        window.update(|s, _, _| {
            assert_eq!(s.tooltip_label().as_deref(), Some("Archive Session ⌘⇧A"));
        });
        // Leaving the action within the still-hovered row must rearm row details.
        let row = window.update(|_, w, _| bounds(w, "session-a").center());
        window.simulate_mouse_move(row);
        window.update(|s, _, _| {
            assert_eq!(s.hint.pending.as_deref(), Some("session-a"));
            assert_eq!(s.hovered_session().as_deref(), Some("a"));
        });
        app.advance_clock(Duration::from_millis(699));
        window.draw();
        window.update(|_, w, _| missing(w, "chrome-tooltip"));
        app.advance_clock(Duration::from_millis(1));
        window.draw();
        window.update(|s, w, _| {
            assert_eq!(s.tooltip_label().as_deref(), Some("a"));
            bounds(w, "chrome-tooltip");
        });
        assert_eq!(*events.borrow(), vec![Action::Archive("a".into())]);
    }
}

#[test]
fn chrome_archived_row_has_details_tooltip_and_open_does_not_restore() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|_, cx| {
        Sidebar::new(
            SidebarProps {
                archived: true,
                sessions: project("first", &["a"]).sessions,
                ..props()
            },
            vec![],
            "1".into(),
            false,
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let received = events.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&cx.entity(), move |_, _, e: &Action, _| {
            received.borrow_mut().push(e.clone())
        })
    });
    window.draw();
    let point = window.update(|_, w, _| bounds(w, "archived-select-a").center());
    window.simulate_mouse_move(point);
    app.advance_clock(Duration::from_millis(699));
    window.draw();
    window.update(|_, w, _| missing(w, "chrome-tooltip"));
    app.advance_clock(Duration::from_millis(1));
    window.draw();
    window.update(|s, w, _| {
        assert_eq!(s.tooltip_label().as_deref(), Some("Archived\na\njust now"));
        bounds(w, "session-tooltip-status");
        bounds(w, "session-tooltip-name");
        bounds(w, "session-tooltip-date");
    });
    window.simulate_click(point, MouseButton::Left);
    window.draw();
    assert_eq!(*events.borrow(), vec![Action::Select("a".into())]);
    let restore = window.update(|_, w, _| bounds(w, "restore-a").center());
    window.simulate_mouse_move(restore);
    app.advance_clock(Duration::from_millis(450));
    window.draw();
    window.update(|s, _, _| assert_eq!(s.tooltip_label().as_deref(), Some("Unarchive Session")));
    window.simulate_click(restore, MouseButton::Left);
    window.draw();
    assert_eq!(
        *events.borrow(),
        vec![Action::Select("a".into()), Action::Restore("a".into())]
    );
}

#[test]
fn chrome_window_deactivation_releases_hold_before_the_next_snapshot() {
    let mut app = gpui_kit::TestAppContext::single();
    app.update(gpui_kit::init);
    let (sidebar, cx) = app.add_window_view(|_, cx| {
        Sidebar::new(
            props(),
            vec![project("first", &["a"])],
            "1".into(),
            false,
            cx,
        )
    });
    cx.update(|w, cx| {
        w.activate_window();
        w.render_frame(cx);
    });
    cx.run_until_parked();
    let pointer = cx.update(|w, _| bounds(w, "sidebar-body").center());
    cx.simulate_event(gpui_kit::MouseMoveEvent {
        position: pointer,
        pressed_button: None,
        modifiers: Default::default(),
    });
    cx.update(|w, cx| w.render_frame(cx));
    cx.update(|_, cx| assert!(sidebar.read(cx).held_session_order.is_some()));
    cx.deactivate_window();
    cx.update(|w, cx| w.render_frame(cx));
    cx.update(|_, cx| {
        assert!(sidebar.read(cx).held_session_order.is_none());
        sidebar.update(cx, |s, cx| {
            s.set_projects(vec![project("first", &["new", "a"])], cx)
        });
    });
    cx.update(|w, cx| {
        w.render_frame(cx);
        bounds(w, "session-status-new");
    });
}

#[test]
fn chrome_read_only_header_does_not_advertise_rename() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window =
        app.open_window(|w, cx| Header::new("a".into(), "Read only".into(), false, w, cx));
    window.update(|s, _, cx| s.set_renamable(false, cx));
    window.draw();
    let point = window.update(|_, w, _| bounds(w, "session-name").center());
    window.simulate_mouse_move(point);
    app.advance_clock(Duration::from_millis(800));
    window.draw();
    window.update(|s, w, _| {
        assert!(s.hint.pending.is_none());
        missing(w, "chrome-tooltip");
    });
    window.simulate_click(point, MouseButton::Left);
    window.draw();
    window.update(|s, _, _| assert!(!s.renaming()));
}
