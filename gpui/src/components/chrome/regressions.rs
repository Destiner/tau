use super::*;

fn bounds(window: &Window, id: &str) -> Bounds<Pixels> {
    gpui_kit::base::test_support::find(window, &[], &id.to_owned().into())
        .unwrap_or_else(|| panic!("missing {id}"))
        .bounds()
}

fn click(
    window: &mut gpui_kit::TestAppWindow<Header>,
    position: Point<Pixels>,
    button: MouseButton,
    count: usize,
) {
    window.simulate_event(MouseDownEvent {
        position,
        button,
        modifiers: Default::default(),
        click_count: count,
        first_mouse: false,
    });
    window.simulate_event(gpui_kit::MouseUpEvent {
        position,
        button,
        modifiers: Default::default(),
        click_count: count,
    });
    window.draw();
}

#[test]
fn header_blank_clicks_route_drag_and_native_double_click_without_focusing() {
    for dark in [false, true] {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app
            .open_window(|w, cx| Header::new("one".into(), "Selected session".into(), dark, w, cx));
        window.draw();
        let positions = window.update(|_, w, _| {
            let header = bounds(w, "header");
            let name = bounds(w, "session-name");
            let new = bounds(w, "new-session");
            [
                Point::new((name.right() + new.left()) / 2., header.center().y),
                Point::new(header.left() + px(2.), header.center().y),
                Point::new(new.right() + px(3.), header.center().y),
            ]
        });
        for position in positions {
            click(&mut window, position, MouseButton::Left, 1);
            click(&mut window, position, MouseButton::Left, 2);
            click(&mut window, position, MouseButton::Right, 2);
            click(&mut window, position, MouseButton::Left, 3);
        }
        window.update(|s, w, cx| {
            assert_eq!(
                s.titlebar_gestures,
                [TitlebarGesture::Drag, TitlebarGesture::DoubleClick].repeat(3)
            );
            assert!(!s.renaming());
            assert!(w.focused(cx).is_none());
        });
    }
}

#[test]
fn header_control_double_clicks_keep_actions_and_never_route_titlebar_gestures() {
    for id in ["new-session", "palette"] {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|w, cx| {
            Header::new("one".into(), "Selected session".into(), false, w, cx)
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = events.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, e: &Action, _| {
                received.borrow_mut().push(e.clone());
            })
        });
        window.draw();
        let position = window.update(|_, w, _| bounds(w, id).center());
        click(&mut window, position, MouseButton::Left, 1);
        click(&mut window, position, MouseButton::Left, 2);
        window.update(|s, _, _| assert!(s.titlebar_gestures.is_empty()));
        assert_eq!(
            *events.borrow(),
            vec![
                if id == "palette" {
                    Action::Palette
                } else {
                    Action::NewSession
                };
                2
            ]
        );
    }
}

#[test]
fn selected_name_click_enters_rename_and_input_double_click_is_not_window_zoom() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window =
        app.open_window(|w, cx| Header::new("one".into(), "Selected session".into(), false, w, cx));
    window.draw();
    let name = window.update(|_, w, _| bounds(w, "session-name").center());
    click(&mut window, name, MouseButton::Left, 1);
    window.update(|s, _, _| assert!(s.renaming()));
    let input = window.update(|_, w, _| bounds(w, "rename-field").center());
    click(&mut window, input, MouseButton::Left, 2);
    window.update(|s, w, cx| {
        assert!(s.renaming());
        assert!(s.editor().read(cx).focus_handle(cx).is_focused(w));
        assert_eq!(s.editor().read(cx).value().as_ref(), "Selected session");
        assert!(s.titlebar_gestures.is_empty());
    });
}

#[test]
fn archive_empty_copy_aligns_with_the_normal_first_content_row() {
    for dark in [false, true] {
        for height in [300., 800.] {
            let mut app = gpui_kit::TestApp::new();
            app.update(gpui_kit::init);
            let props = SidebarProps {
                sessions: vec![],
                active: String::new(),
                collapsed: false,
                present: true,
                archived: false,
            };
            let mut window = app.open_window(|_, cx| {
                Sidebar::new(props.clone(), projects(), "1.2.3".into(), dark, cx)
            });
            window.simulate_resize(gpui_kit::size(px(800.), px(height)));
            window.draw();
            let project = window.update(|_, w, _| bounds(w, "project-row-tau"));
            window.update(|s, _, cx| {
                s.set_props(
                    SidebarProps {
                        archived: true,
                        ..props.clone()
                    },
                    cx,
                );
            });
            window.draw();
            window.update(|_, w, _| {
                let titlebar = bounds(w, "sidebar-blank");
                let empty = bounds(w, "archived-empty");
                assert_eq!(titlebar.size.height, px(30.));
                assert_eq!(empty.top(), titlebar.bottom() + px(6.));
                assert_eq!(empty.left(), project.left());
                assert_eq!(empty.center().y, project.center().y);
                assert_eq!(empty.size.height, project.size.height);
                assert!(empty.bottom() < bounds(w, "sidebar-footer").top());
            });
        }
    }
}

#[test]
fn version_hover_has_no_tooltip_but_click_still_opens_updates() {
    for dark in [false, true] {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|_, cx| {
            Sidebar::new(
                SidebarProps {
                    sessions: vec![],
                    active: String::new(),
                    collapsed: false,
                    present: true,
                    archived: false,
                },
                projects(),
                "1.2.3".into(),
                dark,
                cx,
            )
        });
        window.draw();
        let version = window.update(|_, w, _| bounds(w, "version").center());
        window.simulate_mouse_move(version);
        app.advance_clock(Duration::from_millis(800));
        window.draw();
        window.update(|s, w, _| {
            assert!(s.hint.pending.is_none());
            assert_eq!(s.tooltip_label(), None);
            assert!(gpui_kit::base::test_support::find(w, &[], &"chrome-tooltip".into()).is_none());
        });
        window.simulate_click(version, MouseButton::Left);
        window.draw();
        window.update(|s, w, _| {
            assert!(s.version_open());
            bounds(w, "version-popover");
        });
    }
}

fn projects() -> Vec<ProjectItem> {
    vec![ProjectItem {
        id: "tau".into(),
        title: "Test project".into(),
        location: "/test/project".into(),
        collapsed: false,
        sessions: vec![],
    }]
}

#[test]
fn caller_projects_and_version_have_no_fixture_fallback() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(|_, cx| {
        Sidebar::new(
            SidebarProps {
                sessions: vec![],
                active: String::new(),
                collapsed: false,
                present: true,
                archived: false,
            },
            vec![],
            "9.8.7".into(),
            false,
            cx,
        )
    });
    window.draw();
    window.update(|s, _, _| {
        assert!(s.rendered_projects().is_empty());
        assert_eq!(s.version, "9.8.7");
    });
    window.update(|s, _, cx| s.set_projects(projects(), cx));
    window.draw();
    window.update(|s, w, _| {
        assert_eq!(s.rendered_projects(), projects());
        bounds(w, "project-toggle-tau");
    });
}

#[test]
fn relative_times_and_project_status_preserve_existing_copy_and_priority() {
    assert_eq!(expanded_relative_time("now"), "just now");
    assert_eq!(expanded_relative_time("1m"), "1 minute ago");
    assert_eq!(expanded_relative_time("2h"), "2 hours ago");
    assert_eq!(expanded_relative_time("yesterday"), "yesterday");
    let session = |status| SessionItem {
        id: "one".into(),
        title: "Session".into(),
        time: "now".into(),
        status: Some(status),
        unread: false,
        can_archive: true,
    };
    assert_eq!(
        aggregate_status(&[
            session(SessionStatus::Draft),
            session(SessionStatus::Working)
        ]),
        Some(SessionStatus::Working)
    );
    assert_eq!(
        aggregate_status(&[
            session(SessionStatus::Working),
            session(SessionStatus::Unread)
        ]),
        Some(SessionStatus::Unread)
    );
}
