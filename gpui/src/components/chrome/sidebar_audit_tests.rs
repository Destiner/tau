use super::*;
use gpui_kit::{AppContext, TestApp, TestAppWindow, base::Root, point, size, test::TestWindowExt};

struct Chrome {
    sidebar: Entity<Sidebar>,
    header: Entity<Header>,
}
impl Render for Chrome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.sidebar.clone()).child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.header.clone())
                .child(div().id("main-pane").test_support().flex_1()),
        )
    }
}
fn row() -> SessionItem {
    SessionItem {
        id: "pi-session-id".into(),
        title: "Session".into(),
        time: "2m".into(),
        status: None,
        unread: false,
        can_archive: true,
    }
}
fn props(archived: bool) -> SidebarProps {
    SidebarProps {
        sessions: if archived { vec![row()] } else { vec![] },
        active: String::new(),
        collapsed: false,
        present: true,
        archived,
    }
}
fn fixture(
    dark: bool,
) -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Sidebar>,
    Entity<Header>,
) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let mut entities = None;
    let mut window = app.open_window(|w, cx| {
        let sidebar = cx.new(|cx| {
            Sidebar::new(
                props(false),
                vec![ProjectItem {
                    id: "project-id".into(),
                    title: "Project".into(),
                    location: "/project".into(),
                    collapsed: false,
                    sessions: vec![row()],
                }],
                "0.1.0".into(),
                dark,
                cx,
            )
        });
        let header =
            cx.new(|cx| Header::new("pi-session-id".into(), "Session".into(), dark, w, cx));
        entities = Some((sidebar.clone(), header.clone()));
        let chrome = cx.new(|_| Chrome { sidebar, header });
        Root::new(chrome, w, cx)
    });
    window.simulate_resize(size(px(720.), px(480.)));
    window.simulate_activation(true);
    window.draw();
    let (sidebar, header) = entities.unwrap();
    (app, window, sidebar, header)
}
fn bounds(window: &mut TestAppWindow<Root>, id: &str) -> Bounds<Pixels> {
    window.update(|_, w, _| w.find(id.to_owned()).bounds())
}
fn click(window: &mut TestAppWindow<Root>, id: &str) {
    let position = bounds(window, id).center();
    window.simulate_click(position, MouseButton::Left);
    window.draw();
}
fn painted_wash(window: &Window, bounds: Bounds<Pixels>, dark: bool) -> bool {
    let scaled = Bounds {
        origin: bounds.origin.scale(window.scale_factor()),
        size: bounds.size.scale(window.scale_factor()),
    };
    window
        .painted_quads()
        .iter()
        .any(|q| q.bounds == scaled && q.background == wash(dark).into())
}

#[test]
fn root_header_24px_hit_geometry_hover_disabled_and_project_presence() {
    for dark in [false, true] {
        let (app, mut window, _, header) = fixture(dark);
        let events = Rc::new(RefCell::new(Vec::new()));
        let collected = events.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&header, move |_, _, e: &Action, _| {
                collected.borrow_mut().push(e.clone());
            })
        });
        for id in ["new-session", "palette"] {
            let rect = bounds(&mut window, id);
            assert_eq!(rect.size, size(px(24.), px(24.)));
            let header_rect = bounds(&mut window, "header");
            assert!((rect.top() - header_rect.top() - px(2.5)).abs() <= px(1.));
            // Exercise the newly compact hit box near its bottom/right edge.
            window.simulate_click(
                point(rect.right() - px(0.5), rect.bottom() - px(0.5)),
                MouseButton::Left,
            );
            window.draw();
        }
        assert_eq!(*events.borrow(), vec![Action::NewSession, Action::Palette]);
        events.borrow_mut().clear();
        window.update(|_, _, cx| {
            header.update(cx, |h, cx| h.set_action_admission(true, false, false, cx))
        });
        window.draw();
        for id in ["new-session", "palette"] {
            let rect = bounds(&mut window, id);
            window.simulate_mouse_move(rect.center());
            window.draw();
            window.update(|_, w, _| {
                assert_eq!(w.find(id.to_owned()).disabled(), Some(true));
                assert!(!painted_wash(w, rect, dark));
            });
            click(&mut window, id);
            window.simulate_keystroke("enter");
            window.simulate_keystroke("space");
        }
        assert!(events.borrow().is_empty());
        window.update(|_, _, cx| {
            header.update(cx, |h, cx| h.set_action_admission(true, true, true, cx))
        });
        window.draw();
        let rect = bounds(&mut window, "palette");
        window.simulate_mouse_move(point(px(700.), px(400.)));
        window.draw();
        window.simulate_mouse_move(rect.center());
        app.advance_clock(Duration::from_millis(449));
        window.draw();
        window.update(|_, _, cx| assert!(header.read(cx).tooltip_label().is_none()));
        app.advance_clock(Duration::from_millis(1));
        window.draw();
        window.update(|_, _, cx| {
            assert_eq!(
                header.read(cx).tooltip_label().as_deref(),
                Some("Command Palette ⌘K")
            )
        });
        window.update(|_, _, cx| {
            header.update(cx, |h, cx| h.set_action_admission(false, false, true, cx))
        });
        window.draw();
        window.update(|_, w, _| {
            assert!(gpui_kit::base::test_support::find(w, &[], &"new-session".into()).is_none())
        });
        click(&mut window, "palette");
        assert_eq!(*events.borrow(), vec![Action::Palette]);
    }
}

#[test]
fn root_sidebar_disabled_actions_context_drag_and_truthful_removal_boundary() {
    for dark in [false, true] {
        let (app, mut window, sidebar, _) = fixture(dark);
        let events = Rc::new(RefCell::new(Vec::new()));
        let collected = events.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&sidebar, move |_, _, e: &Action, _| {
                collected.borrow_mut().push(e.clone());
            })
        });
        window.update(|_, _, cx| {
            sidebar.update(cx, |s, cx| {
                let mut projects = s.projects.clone();
                projects[0].title = "Long project name ".repeat(100);
                s.set_projects(projects, cx);
            })
        });
        window.draw();
        let label = bounds(&mut window, "project-label-project-id");
        let toggle = bounds(&mut window, "project-toggle-project-id");
        let new = bounds(&mut window, "project-new-project-id");
        assert!(label.right() <= toggle.right());
        assert!(toggle.right() <= new.left());
        assert!(new.right() <= bounds(&mut window, "sidebar").right());
        let project_row = bounds(&mut window, "project-row-project-id");
        window.simulate_mouse_move(project_row.center());
        window.draw();
        window.update(|_, w, _| assert!(painted_wash(w, project_row, dark)));
        let remove = bounds(&mut window, "project-remove-project-id");
        window.simulate_mouse_move(remove.center());
        app.advance_clock(Duration::from_millis(450));
        window.draw();
        window.update(|_, w, cx| {
            assert_eq!(w.find("project-remove-project-id").disabled(), Some(true));
            assert_eq!(
                sidebar.read(cx).tooltip_label().as_deref(),
                Some("Remove Project unavailable in this build")
            );
        });
        click(&mut window, "project-remove-project-id");
        assert!(
            events.borrow().is_empty(),
            "unsupported removal must not claim success or toggle the project"
        );
        window.update(|_, _, cx| sidebar.update(cx, |s, cx| s.set_actions_interactive(false, cx)));
        window.draw();
        window.simulate_mouse_move(project_row.center());
        window.draw();
        window.update(|_, w, _| assert!(!painted_wash(w, project_row, dark)));
        for id in [
            "project-toggle-project-id",
            "project-new-project-id",
            "session-pi-session-id",
            "archive-pi-session-id",
            "open-project",
        ] {
            let rect = bounds(&mut window, id);
            window.simulate_mouse_move(rect.center());
            window.draw();
            window.update(|_, w, _| {
                assert_eq!(w.find(id.to_owned()).disabled(), Some(true));
                assert!(!painted_wash(w, rect, dark));
            });
            click(&mut window, id);
        }
        let session = bounds(&mut window, "session-pi-session-id").center();
        window.simulate_click(session, MouseButton::Right);
        window.draw();
        let drag = bounds(&mut window, "project-drag-project-id").center();
        window.simulate_mouse_down(drag, MouseButton::Left);
        window.simulate_mouse_move(point(drag.x, drag.y + px(50.)));
        window.simulate_mouse_up(drag, MouseButton::Left);
        window.draw();
        window.update(|_, _, cx| {
            let s = sidebar.read(cx);
            assert!(!s.context_menu.read(cx).is_open());
            assert!(s.project_drag.is_none());
        });
        assert!(events.borrow().is_empty());
        window.update(|_, _, cx| {
            sidebar.update(cx, |s, cx| {
                s.set_actions_interactive(true, cx);
                s.set_remove_project_available(true, cx);
            })
        });
        window.draw();
        click(&mut window, "project-remove-project-id");
        click(&mut window, "session-pi-session-id");
        assert_eq!(
            *events.borrow(),
            vec![
                Action::RemoveProject("project-id".into()),
                Action::Select("pi-session-id".into())
            ]
        );
        events.borrow_mut().clear();
        window.update(|_, _, cx| {
            sidebar.update(cx, |s, cx| {
                s.set_props(props(true), cx);
                s.set_actions_interactive(false, cx);
            })
        });
        window.draw();
        for id in ["archived-select-pi-session-id", "restore-pi-session-id"] {
            click(&mut window, id);
            window.update(|_, w, _| assert_eq!(w.find(id.to_owned()).disabled(), Some(true)));
        }
        assert!(events.borrow().is_empty());
    }
}

#[test]
fn business_admission_and_width_do_not_replace_version_modal_policy() {
    let (_app, mut window, sidebar, _) = fixture(false);
    click(&mut window, "version");
    window.update(|_, w, cx| {
        sidebar.update(cx, |s, cx| {
            assert!(s.version_open());
            s.set_actions_interactive(false, cx);
            s.set_width(300., cx);
            assert!(s.version_open());
            assert!(s.version_interactive);
            assert!(s.version_surface_focus.is_focused(w));
            s.preserve_version_under_palette(cx);
            s.set_actions_interactive(true, cx);
            s.set_remove_project_available(true, cx);
            assert!(s.version_open());
            assert!(!s.version_interactive);
            s.set_version_interactive(false, cx);
            assert!(!s.version_open());
        })
    });
    window.draw();
}

#[test]
fn disabled_icon_presentation_does_not_reuse_the_enabled_hover_opacity() {
    for dark in [false, true] {
        let mut enabled = icon_button_admitted(
            "enabled",
            "plus",
            28.,
            dark,
            false,
            (support::tokens(dark).muted, true),
        );
        let mut disabled = icon_button_admitted(
            "disabled",
            "plus",
            28.,
            dark,
            false,
            (support::tokens(dark).muted, false),
        );
        assert_eq!(enabled.style().opacity, Some(0.65));
        assert_eq!(disabled.style().opacity, Some(0.35));
    }
}

#[test]
fn root_width_hydration_is_bounded_silent_and_resize_emits_changed_values_only() {
    let (_app, mut window, sidebar, _) = fixture(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let collected = events.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&sidebar, move |_, _, e: &SidebarWidthChanged, _| {
            collected.borrow_mut().push(e.0);
        })
    });
    for (input, expected) in [
        (1., 200.),
        (900., 480.),
        (337.6, 338.),
        (f32::NAN, 260.),
        (f32::INFINITY, 260.),
    ] {
        window.update(|_, _, cx| sidebar.update(cx, |s, cx| s.set_width(input, cx)));
        window.draw();
        assert_eq!(bounds(&mut window, "sidebar").size.width, px(expected));
        window.read(|_, cx| assert_eq!(sidebar.read(cx).width(), expected));
    }
    assert!(events.borrow().is_empty());
    window.update(|_, w, cx| {
        let focus = sidebar.read(cx).resize_focus.clone();
        focus.focus(w, cx);
    });
    window.draw();
    window.update(|_, w, cx| {
        assert!(sidebar.read(cx).handle_highlight(w));
        assert!(w.find("sidebar-resize-line").visible());
    });
    for key in ["right", "shift-right", "home", "left", "end", "right"] {
        window.simulate_keystroke(key);
        window.draw();
    }
    assert_eq!(*events.borrow(), vec![270., 310., 200., 480.]);
    let grip = bounds(&mut window, "sidebar-resize-handle").center();
    window.simulate_mouse_down(grip, MouseButton::Left);
    window.draw();
    window.simulate_mouse_up(point(px(333.6), grip.y), MouseButton::Left);
    window.draw();
    assert_eq!(events.borrow().last(), Some(&334.));
    assert_eq!(bounds(&mut window, "sidebar").size.width, px(334.));
    assert_eq!(bounds(&mut window, "header").left(), px(334.));
    window.read(|_, cx| assert!(!sidebar.read(cx).resizing()));
}

#[test]
fn root_rich_tooltip_is_viewport_bounded_scrollable_and_pointer_reachable() {
    for dark in [false, true] {
        for viewport in [size(px(720.), px(480.)), size(px(360.), px(240.))] {
            let (mut app, mut window, sidebar, _) = fixture(dark);
            window.simulate_resize(viewport);
            window.update(|_, w, cx| {
                w.bounds_changed(cx);
                assert_eq!(w.viewport_size(), viewport);
                sidebar.update(cx, |s, cx| {
                    s.set_session_details(
                        BTreeMap::from([(
                            "pi-session-id".into(),
                            SessionDetails {
                                status: "Idle".into(),
                                message: "**Long title**\n\n".repeat(100),
                                relative_time: Some("2m".into()),
                            },
                        )]),
                        cx,
                    );
                })
            });
            window.draw();
            let anchor = bounds(&mut window, "session-pi-session-id");
            app.update(|cx| {
                cx.update_window(window.handle().into(), |_, w, cx| {
                    w.hover("session-pi-session-id", cx)
                })
                .unwrap();
            });
            app.advance_clock(Duration::from_millis(699));
            window.draw();
            window.update(|_, _, cx| assert!(sidebar.read(cx).tooltip_label().is_none()));
            app.advance_clock(Duration::from_millis(1));
            window.draw();
            let tip = bounds(&mut window, "chrome-tooltip");
            assert!(tip.size.width <= px(300.));
            assert!(tip.left() >= px(6.) && tip.right() <= viewport.width - px(6.));
            assert!(
                tip.top() >= px(6.) && tip.bottom() <= viewport.height - px(6.),
                "tooltip {tip:?}, anchor {anchor:?}, viewport {viewport:?}"
            );
            let gap = if tip.top() >= anchor.bottom() {
                tip.top() - anchor.bottom()
            } else {
                anchor.top() - tip.bottom()
            };
            assert!((f32::from(gap) - 6.).abs() <= 1.);
            let before = bounds(&mut window, "session-tooltip-date").top();
            assert!(
                before > tip.bottom(),
                "long tooltip needs real overflow, not truncated content"
            );
            app.update(|cx| {
                cx.update_window(window.handle().into(), |_, w, cx| {
                    w.hover("chrome-tooltip", cx)
                })
                .unwrap();
            });
            app.advance_clock(Duration::from_millis(100));
            window.draw();
            window.read(|_, cx| assert!(sidebar.read(cx).tooltip_label().is_some()));
            app.update(|cx| {
                cx.update_window(window.handle().into(), |_, w, cx| {
                    w.scroll(
                        "chrome-tooltip",
                        gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-200.))),
                        cx,
                    );
                })
                .unwrap();
            });
            let after = bounds(&mut window, "session-tooltip-date").top();
            assert!(after < before, "wheel must scroll the tooltip's own reader");
            assert_eq!(bounds(&mut window, "chrome-tooltip"), tip);
            app.update(|cx| {
                cx.update_window(window.handle().into(), |_, w, cx| w.hover("main-pane", cx))
                    .unwrap();
            });
            app.advance_clock(Duration::from_millis(100));
            window.draw();
            window.read(|_, cx| assert!(sidebar.read(cx).tooltip_label().is_none()));
        }
    }
}
