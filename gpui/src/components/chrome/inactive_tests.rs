use super::sidebar_audit_tests::{bounds, click, fixture};
use super::*;
use gpui_kit::{MouseExitEvent, point, test::TestWindowExt};

fn vue_selection(dark: bool, active: bool) -> gpui_kit::Rgba {
    rgba(match (dark, active) {
        (false, true) => 0x6b7d8f33,
        (false, false) => 0x6b7d8f1c,
        (true, true) => 0x4752664d,
        (true, false) => 0x47526626,
    })
}

fn assert_selection_paint(window: &Window, id: &str, selection: gpui_kit::Rgba, opacity: f32) {
    let b = window.find(id.to_owned()).bounds();
    let scaled = Bounds {
        origin: b.origin.scale(window.scale_factor()),
        size: b.size.scale(window.scale_factor()),
    };
    let expected = gpui_kit::Background::from(selection).opacity(opacity);
    let painted = window
        .painted_quads()
        .into_iter()
        .filter(|q| q.bounds == scaled)
        .collect::<Vec<_>>();
    assert!(
        painted.iter().any(|q| q.background == expected),
        "{id}: active={}, expected={expected:?}, painted={painted:?}",
        window.is_window_active()
    );
}

#[test]
fn row_leave_invalidates_without_tooltip_timer_or_unrelated_input() {
    for dark in [false, true] {
        for exit in ["blank", "main", "window", "deactivate"] {
            let (_app, mut window, sidebar, _) = fixture(dark);
            let notifications = Rc::new(Cell::new(0));
            let count = notifications.clone();
            let _observer = window
                .update(|_, _, cx| cx.observe(&sidebar, move |_, _, _| count.set(count.get() + 1)));
            let row = bounds(&mut window, "session-row-pi-session-id");
            let archive = bounds(&mut window, "archive-pi-session-id");
            let row_point = point(row.left() + px(40.), row.center().y);
            assert!(row.contains(&row_point));
            assert!(!archive.contains(&row_point));
            window.simulate_mouse_move(row_point);
            window.draw();
            window.update(|_, w, cx| {
                assert!(sidebar.read(cx).session_hover.contains("pi-session-id"));
                assert!(w.find("archive-pi-session-id").visible());
                assert!(!w.find("session-time-pi-session-id").visible());
            });
            let before = notifications.get();
            match exit {
                "blank" => {
                    window.simulate_mouse_move(point(row.left() + px(40.), row.bottom() + px(80.)))
                }
                "main" => {
                    let main = bounds(&mut window, "main-pane");
                    assert!(!row.contains(&main.center()));
                    window.simulate_mouse_move(main.center());
                }
                "window" => window.simulate_event(MouseExitEvent {
                    position: point(px(-1.), row.center().y),
                    pressed_button: None,
                    modifiers: Default::default(),
                }),
                "deactivate" => window.simulate_activation(false),
                _ => unreachable!(),
            }
            // Verify invalidation BEFORE drawing. A forced frame must not conceal a
            // missing leave notification (the former test-only hover tracking did).
            assert!(
                notifications.get() > before,
                "{exit} must dirty the owning view"
            );
            window.read(|_, cx| assert!(sidebar.read(cx).session_hover.is_empty()));
            window.draw();
            window.update(|_, w, _| {
                assert!(!w.find("archive-pi-session-id").visible(), "{exit}");
                assert!(w.find("session-time-pi-session-id").visible(), "{exit}");
            });
            if exit == "deactivate" {
                window.simulate_activation(true);
            }
            window.simulate_mouse_move(row_point);
            window.draw();
            window.update(|_, w, _| {
                assert!(
                    w.find("archive-pi-session-id").visible(),
                    "re-entry after {exit}"
                )
            });
        }
    }
}

#[test]
fn rapid_enter_exit_and_keyboard_input_do_not_invent_or_cancel_row_hover() {
    for dark in [false, true] {
        let (_app, mut window, sidebar, _) = fixture(dark);
        let row = bounds(&mut window, "session-row-pi-session-id");
        let main = bounds(&mut window, "main-pane");
        let pointer = point(row.left() + px(40.), row.center().y);
        for _ in 0..4 {
            window.simulate_mouse_move(pointer);
            window.simulate_mouse_move(main.center());
        }
        window.read(|_, cx| assert!(sidebar.read(cx).session_hover.is_empty()));
        window.draw();
        window.update(|_, w, _| assert!(!w.find("archive-pi-session-id").visible()));
        // A legitimate stationary pointer remains hover even when keyboard modality changes.
        window.simulate_mouse_move(pointer);
        window.draw();
        window.simulate_keystroke("shift");
        window.draw();
        window.read(|_, cx| assert!(sidebar.read(cx).session_hover.contains("pi-session-id")));
        window.update(|_, w, _| assert!(w.find("archive-pi-session-id").visible()));
        // Crossing into the action itself stays inside the measured row.
        let archive = bounds(&mut window, "archive-pi-session-id");
        assert!(row.contains(&archive.center()));
        window.simulate_mouse_move(archive.center());
        window.draw();
        window.read(|_, cx| assert!(sidebar.read(cx).session_hover.contains("pi-session-id")));
    }
}

#[test]
fn native_activation_changes_only_selection_wash_and_preserves_header_editor() {
    for dark in [false, true] {
        for warm in [false, true] {
            let (_app, mut window, sidebar, header) = fixture(dark);
            window.update(|_, _, cx| {
                sidebar.update(cx, |s, cx| {
                    s.set_active_session("pi-session-id".into(), cx);
                })
            });
            if warm {
                click(&mut window, "session-name");
                window.simulate_keystroke("end");
                window.simulate_input(" draft");
                window.draw();
                window.update(|_, w, cx| {
                    let h = header.read(cx);
                    assert!(h.renaming(), "typing must not close the editor");
                    assert!(h.input.read(cx).focus_handle(cx).is_focused(w));
                    assert_eq!(h.input.read(cx).value(), "Session draft");
                });
            }
            let editor = window.read(|_, cx| header.read(cx).input.clone());
            let editor_id = editor.entity_id();
            let before = window.read(|_, cx| editor.read(cx).value());
            let style = window.read(|_, cx| header.read(cx).input_style.clone());
            let notifications = Rc::new(Cell::new(0));
            let count = notifications.clone();
            let _observer = window
                .update(|_, _, cx| cx.observe(&sidebar, move |_, _, _| count.set(count.get() + 1)));
            for active in [false, true, false] {
                let before_notify = notifications.get();
                window.simulate_activation(active);
                assert!(
                    notifications.get() > before_notify,
                    "activation must invalidate cached paint"
                );
                window.draw();
                window.update(|_, w, cx| {
                    assert_eq!(w.is_window_active(), active);
                    let t = support::tokens_for_window(dark, w);
                    assert_eq!(
                        t.selected,
                        match (dark, active) {
                            (false, true) => 0x6b7d8f33,
                            (false, false) => 0x6b7d8f1c,
                            (true, true) => 0x4752664d,
                            (true, false) => 0x47526626,
                        }
                    );
                    let ordinary = support::tokens(dark);
                    assert_eq!(t.text, ordinary.text);
                    assert_eq!(t.accent, ordinary.accent);
                    assert_eq!(t.hover, ordinary.hover);
                    assert_eq!(t.scrim, ordinary.scrim);
                    let b = w.find("session-pi-session-id").bounds();
                    let scaled = Bounds {
                        origin: b.origin.scale(w.scale_factor()),
                        size: b.size.scale(w.scale_factor()),
                    };
                    assert!(
                        w.painted_quads()
                            .iter()
                            .any(|q| q.bounds == scaled && q.background == rgba(t.selected).into())
                    );
                    let h = header.read(cx);
                    assert_eq!(h.input.entity_id(), editor_id);
                    assert_eq!(h.input.read(cx).value(), before);
                    assert_eq!(h.input_style.selection, style.selection);
                    assert_eq!(h.input_style.foreground, style.foreground);
                    assert_eq!(h.editing.is_some(), warm);
                    if warm {
                        assert!(h.input.read(cx).focus_handle(cx).is_focused(w));
                    }
                });
            }
        }
    }
}

#[test]
fn archived_rows_and_toggle_dim_but_open_project_does_not_gain_selection() {
    for dark in [false, true] {
        for archived in [false, true] {
            let (_app, mut window, sidebar, _) = fixture(dark);
            let targets = if archived {
                window.update(|_, _, cx| {
                    sidebar.update(cx, |s, cx| {
                        let session = s.projects[0].sessions[0].clone();
                        s.set_props(
                            SidebarProps {
                                sessions: vec![session],
                                active: "pi-session-id".into(),
                                collapsed: false,
                                present: true,
                                archived: true,
                            },
                            cx,
                        );
                    })
                });
                window.draw();
                vec!["archived-row-pi-session-id", "archived"]
            } else {
                click(&mut window, "open-project");
                vec!["open-project"]
            };
            for active in [false, true] {
                window.simulate_activation(active);
                window.draw();
                window.update(|_, w, _| {
                    assert_eq!(w.is_window_active(), active);
                    let selection = vue_selection(dark, active);
                    assert_eq!(
                        rgba(support::tokens_for_window(dark, w).selected),
                        selection
                    );
                    for id in &targets {
                        let b = w.find(*id).bounds();
                        let scaled = Bounds {
                            origin: b.origin.scale(w.scale_factor()),
                            size: b.size.scale(w.scale_factor()),
                        };
                        // Vue's default fade composites the entire archive toggle at .65;
                        // the selected class supplies a background, not an opacity override.
                        let opacity = if *id == "archived" { 0.65 } else { 1. };
                        let expected = gpui_kit::Background::from(selection).opacity(opacity);
                        assert_eq!(
                            w.painted_quads()
                                .iter()
                                .any(|q| q.bounds == scaled && q.background == expected),
                            archived,
                            "{id}: active={active}, painted={:?}",
                            w.painted_quads()
                                .iter()
                                .filter(|q| q.bounds == scaled)
                                .collect::<Vec<_>>()
                        );
                    }
                });
            }
        }
    }
}

#[test]
fn selected_archive_toggle_keeps_its_wash_under_hover() {
    for dark in [false, true] {
        let (_app, mut window, sidebar, _) = fixture(dark);
        window.update(|_, _, cx| {
            sidebar.update(cx, |s, cx| {
                let mut props = s.props.clone();
                props.archived = true;
                s.set_props(props, cx);
            });
        });
        window.draw();
        let toggle = bounds(&mut window, "archived");
        let main = bounds(&mut window, "main-pane");
        for active in [true, false, true] {
            window.simulate_activation(active);
            for pointer in [main.center(), toggle.center(), main.center()] {
                window.simulate_mouse_move(pointer);
                window.draw();
                window.update(|_, w, _| {
                    assert_eq!(w.is_window_active(), active);
                    let selection = vue_selection(dark, active);
                    assert_eq!(
                        rgba(support::tokens_for_window(dark, w).selected),
                        selection
                    );
                    let opacity = if toggle.contains(&pointer) { 1. } else { 0.65 };
                    assert_selection_paint(w, "archived", selection, opacity);
                });
            }
        }
    }
}

#[test]
fn selected_archive_toggle_keeps_selection_through_real_tab_space_and_activation() {
    for dark in [false, true] {
        let (_app, mut window, sidebar, _) = fixture(dark);
        let actions = Rc::new(RefCell::new(Vec::new()));
        let recorded = actions.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&sidebar, move |_, _, action: &Action, _| {
                recorded.borrow_mut().push(action.clone());
            })
        });
        window.update(|_, _, cx| {
            sidebar.update(cx, |s, cx| {
                let mut props = s.props.clone();
                props.archived = true;
                s.set_props(props, cx);
            });
        });
        window.draw();
        let main = bounds(&mut window, "main-pane");
        window.simulate_mouse_move(main.center());
        // Traverse the actual Root tab order; never manufacture focus after input.
        for _ in 0..24 {
            window.simulate_keystroke("tab");
            window.draw();
            if window.update(|_, w, _| w.find("archived").focused() == Some(true)) {
                break;
            }
        }
        window.update(|_, w, _| {
            assert_eq!(w.find("archived").focused(), Some(true));
            assert_selection_paint(w, "archived", vue_selection(dark, true), 1.);
        });
        let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
        window.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        window.draw();
        assert_eq!(*actions.borrow(), vec![Action::ToggleArchived]);
        for active in [false, true] {
            window.simulate_activation(active);
            window.draw();
            window.update(|_, w, _| {
                assert_eq!(w.is_window_active(), active);
                assert_eq!(w.find("archived").focused(), Some(true));
                assert_selection_paint(w, "archived", vue_selection(dark, active), 1.);
            });
        }
    }
}

#[test]
fn deactivation_cancels_pending_and_visible_header_and_sidebar_tooltips() {
    for dark in [false, true] {
        for visible in [false, true] {
            let (app, mut window, sidebar, header) = fixture(dark);
            let row = bounds(&mut window, "session-row-pi-session-id");
            window.simulate_mouse_move(point(row.left() + px(40.), row.center().y));
            if visible {
                app.advance_clock(Duration::from_millis(710));
                window.draw();
            }
            window.simulate_activation(false);
            app.advance_clock(Duration::from_millis(800));
            window.draw();
            window.read(|_, cx| {
                assert!(sidebar.read(cx).hint.pending.is_none());
                assert!(sidebar.read(cx).hint.visible.is_none());
            });
            window.simulate_activation(true);
            let palette = bounds(&mut window, "palette");
            assert!(!row.contains(&palette.center()));
            window.simulate_mouse_move(palette.center());
            if visible {
                app.advance_clock(Duration::from_millis(460));
                window.draw();
            }
            window.simulate_activation(false);
            app.advance_clock(Duration::from_millis(800));
            window.draw();
            window.read(|_, cx| {
                assert!(header.read(cx).hint.pending.is_none());
                assert!(header.read(cx).hint.visible.is_none());
            });
        }
    }
}
