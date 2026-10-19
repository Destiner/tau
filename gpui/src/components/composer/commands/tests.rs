use super::*;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, base::Root, test::TestWindowExt};
use std::cell::RefCell;

struct Host {
    composer: Entity<Composer>,
    top: f32,
    fill: bool,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().relative().child(
            div()
                .absolute()
                .top(px(self.top))
                .left(px(12.))
                .right(px(12.))
                .when(self.fill, |d| d.bottom(px(12.)))
                .child(self.composer.clone()),
        )
    }
}
fn option(name: &str, source: CommandSource) -> CommandOption {
    CommandOption {
        name: name.into(),
        description: Some(format!("Description of {name}")),
        source,
        source_info: None,
    }
}
fn catalog() -> Vec<CommandOption> {
    vec![
        option("session-name", CommandSource::Extension),
        option("summarize", CommandSource::Prompt),
        option("skill:source-check", CommandSource::Skill),
    ]
}
fn fixture(
    top: f32,
    fill: bool,
    catalog: Vec<CommandOption>,
) -> (TestApp, TestAppWindow<Root>, Entity<Composer>) {
    let mut app = TestApp::new();
    app.update(|cx| super::super::super::init(cx, false));
    let mut composer = None;
    let mut window = app.open_window_with_options(
        gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::new(
                Default::default(),
                gpui_kit::size(px(640.), px(600.)),
            ))),
            ..Default::default()
        },
        |window, cx| {
            let host = cx.new(|cx| {
                let entity = cx.new(|cx| {
                    let mut s = Composer::new(
                        ComposerOptions {
                            models: vec![],
                            model_heading: String::new(),
                            efforts: vec![],
                            model: String::new(),
                            effort: String::new(),
                            draft: String::new(),
                            dark: false,
                            disabled: false,
                            streaming: false,
                        },
                        window,
                        cx,
                    );
                    s.set_fill_height(fill, cx);
                    s.set_command_top_boundary(px(40.), cx);
                    s.set_commands(catalog, cx);
                    s
                });
                composer = Some(entity.clone());
                Host {
                    composer: entity,
                    top,
                    fill,
                }
            });
            Root::new(host, window, cx)
        },
    );
    window.draw();
    let composer = composer.unwrap();
    window.update(|_, w, cx| {
        composer.update(cx, |s, cx| s.editor.update(cx, |e, cx| e.focus(w, cx)))
    });
    (app, window, composer)
}
fn draft(window: &TestAppWindow<Root>, composer: &Entity<Composer>) -> String {
    window.read(|_, cx| composer.read(cx).editor.read(cx).value().to_string())
}
fn active(window: &TestAppWindow<Root>, composer: &Entity<Composer>) -> Option<String> {
    window.read(|_, cx| composer.read(cx).active_command().map(|c| c.name.clone()))
}
fn events(
    window: &mut TestAppWindow<Root>,
    composer: &Entity<Composer>,
) -> (Rc<RefCell<Vec<Event>>>, Subscription) {
    let received = Rc::new(RefCell::new(vec![]));
    let output = received.clone();
    let subscription = window.update(|_, _, cx| {
        cx.subscribe(composer, move |_, _, e: &Event, _| {
            output.borrow_mut().push(e.clone())
        })
    });
    (received, subscription)
}
fn paint(window: &mut TestAppWindow<Root>) {
    window.draw();
    for _ in 0..2 {
        window.update(|_, w, cx| {
            w.simulate_next_frame(cx);
        });
        window.draw();
    }
}
fn set_draft(window: &mut TestAppWindow<Root>, composer: &Entity<Composer>, text: &str) {
    window.update(|_, w, cx| {
        composer.update(cx, |s, cx| {
            s.editor.update(cx, |e, cx| e.set_value(text, w, cx))
        })
    });
    paint(window);
}
fn assert_focus(window: &mut TestAppWindow<Root>, composer: &Entity<Composer>) {
    window.update(|_, w, cx| {
        assert!(
            composer
                .read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
}
#[test]
fn vue_queries_ranking_identity_and_pi_wire_options() {
    for (value, expected) in [
        ("/", Some("")),
        ("/ssn", Some("ssn")),
        (" /ssn", None),
        ("/session-name ", None),
        ("/session-name value", None),
        ("/session\n", None),
        ("//", None),
    ] {
        assert_eq!(slash_query(value), expected);
    }
    let mut state = Commands {
        options: catalog(),
        ..Default::default()
    };
    for (query, expected) in [
        ("/", vec![0, 1, 2]),
        ("/ssn", vec![0]),
        ("/sum", vec![1]),
        ("/sc", vec![2]),
        ("/SUM", vec![1]),
        ("/not-found", vec![]),
    ] {
        state.sync(query);
        assert_eq!(state.indices(), expected);
    }
    let option: CommandOption = serde_json::from_value(serde_json::json!({
        "name":"skill:source-check", "source":"skill", "description":"Check a claim",
        "sourceInfo":{"path":"/fixture/skill", "source":"local", "scope":"project", "origin":"top-level", "baseDir":"/fixture"}
    })).unwrap();
    assert_eq!(option.source, CommandSource::Skill);
    assert_eq!(option.source_info.unwrap().origin, CommandOrigin::TopLevel);
}
#[test]
fn unchanged_query_reuses_matches_and_catalog_changes_refilter() {
    let mut state = Commands {
        options: catalog(),
        ..Default::default()
    };
    state.sync("/sum");
    assert_eq!(state.indices(), &[1]);
    let allocation = state.indices().as_ptr();
    state.cursor = 1;
    state.reveal.set(false);
    for _ in 0..10 {
        state.sync("/sum");
        assert_eq!(state.indices().as_ptr(), allocation);
        assert_eq!(state.cursor, 1);
        assert!(!state.reveal.get());
    }
    state.options = vec![option("summarize", CommandSource::Prompt)];
    state.refilter();
    assert_eq!(state.indices(), &[0]);
    state.sync("ordinary");
    assert!(state.indices().is_empty());
}

#[test]
fn root_typing_tab_history_selection_and_enter_once_after_reattach() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    let editor = window.read(|_, cx| composer.read(cx).editor());
    let (received, _subscription) = events(&mut window, &composer);
    for reattach in [false, true] {
        set_draft(&mut window, &composer, "");
        window.update(|_, w, cx| {
            composer.update(cx, |s, cx| {
                if reattach {
                    s.reattach_window(w, cx);
                }
                s.editor.update(cx, |e, cx| e.focus(w, cx));
            })
        });
        window.simulate_input("/");
        window.draw();
        assert_eq!(active(&window, &composer).as_deref(), Some("session-name"));
        window.simulate_keystroke("down");
        window.simulate_keystroke("tab");
        window.draw();
        assert_eq!(draft(&window, &composer), "/summarize ");
        assert!(received.borrow().is_empty());
        window.read(|_, cx| assert_eq!(editor.read(cx).selected_range(), 11..11));
        window.simulate_keystroke("cmd-z");
        window.draw();
        assert_eq!(draft(&window, &composer), "/");
        window.simulate_keystroke("cmd-shift-z");
        window.draw();
        assert_eq!(draft(&window, &composer), "/summarize ");
        set_draft(&mut window, &composer, "/sc");
        window.simulate_keystroke("enter");
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![Event::Send("/skill:source-check".into())]
        );
        assert_eq!(draft(&window, &composer), "");
        received.borrow_mut().clear();
        window.update(|_, _, cx| {
            composer.update(cx, |s, cx| {
                s.set_readiness(true, false, false, false, false, cx)
            })
        });
        assert_focus(&mut window, &composer);
        window.read(|_, cx| assert_eq!(composer.read(cx).editor(), editor));
    }
}
#[test]
fn root_shift_newline_followup_and_streaming_extension_admission() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    let (received, _subscription) = events(&mut window, &composer);
    set_draft(&mut window, &composer, "/sum");
    window.update(|_, _, cx| {
        composer
            .read(cx)
            .editor()
            .update(cx, |e, cx| e.set_selected_range(4..4, cx))
    });
    window.simulate_keystroke("shift-enter");
    window.draw();
    assert_eq!(draft(&window, &composer), "/sum\n");
    assert!(received.borrow().is_empty());
    for streaming in [false, true] {
        window.update(|_, _, cx| composer.update(cx, |s, cx| s.set_status(false, streaming, cx)));
        set_draft(&mut window, &composer, "/sum");
        window.simulate_keystroke(if cfg!(target_os = "macos") {
            "cmd-enter"
        } else {
            "ctrl-enter"
        });
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![Event::FollowUp("/summarize".into())]
        );
        received.borrow_mut().clear();
        // Model a finished controller acknowledgment before the next independent gesture.
        window.update(|_, _, cx| {
            composer.update(cx, |s, cx| {
                s.set_readiness(true, false, false, false, false, cx)
            })
        });
    }
    set_draft(&mut window, &composer, "/ssn");
    window.simulate_keystroke("enter");
    window.draw();
    assert!(received.borrow().is_empty());
    assert_eq!(draft(&window, &composer), "/session-name");
    assert_focus(&mut window, &composer);
    // Completion remains admitted, but submitting extension arguments while streaming is not.
    window.simulate_keystroke("tab");
    window.simulate_input("a name");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(draft(&window, &composer), "/session-name a name");
    assert!(received.borrow().is_empty());
    // Pi splits only on ASCII space, not a newline.
    set_draft(&mut window, &composer, "/session-name\ntext");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(
        *received.borrow(),
        vec![Event::Send("/session-name\ntext".into())]
    );
}
#[test]
fn root_escape_stays_dismissed_until_token_exit_and_host_set_value_resets_cursor() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    window.simulate_input("/");
    window.draw();
    window.simulate_keystroke("escape");
    window.simulate_input("s");
    window.draw();
    assert!(active(&window, &composer).is_none());
    set_draft(&mut window, &composer, "/sum");
    assert!(active(&window, &composer).is_none());
    set_draft(&mut window, &composer, "ordinary");
    set_draft(&mut window, &composer, "/");
    window.simulate_keystroke("down");
    window.draw();
    assert_eq!(active(&window, &composer).as_deref(), Some("summarize"));
    set_draft(&mut window, &composer, "/sc");
    assert_eq!(
        active(&window, &composer).as_deref(),
        Some("skill:source-check")
    );
    assert_focus(&mut window, &composer);
}
#[test]
fn command_cursor_and_hover_paint_wash_and_rail_without_moving_first_line() {
    for dark in [false, true] {
        let (_app, mut window, composer) = fixture(40., true, catalog());
        window.update(|_, w, cx| composer.update(cx, |c, cx| c.set_theme(dark, w, cx)));
        window.simulate_input("/");
        window.draw();
        let editor = window.update(|_, w, _| w.find("editor").bounds());
        let popup = window.update(|_, w, _| w.find("command-menu").bounds());
        assert_eq!(popup.top(), editor.top() + px(28.));
        let first = window.update(|_, w, _| w.find("command-copy-0").bounds());
        for hovered in [false, true] {
            if hovered {
                let at = window.update(|_, w, _| w.find("command-option-1").bounds().center());
                window.simulate_mouse_move(at);
                window.draw();
            }
            window.update(|_, w, _| {
                assert_eq!(w.find("command-copy-0").bounds(), first);
                assert_eq!(w.find("command-menu").bounds(), popup);
                let row = w
                    .find(if hovered {
                        "command-option-1"
                    } else {
                        "command-option-0"
                    })
                    .bounds();
                let row = w.pixel_snap_bounds(row).scale(w.scale_factor());
                let wash = rgb(if dark { 0x475266 } else { 0x6b7d8f }).opacity(if dark {
                    0.2
                } else {
                    0.1
                });
                assert!(
                    w.painted_quads()
                        .iter()
                        .any(|q| q.background == wash.into() && q.bounds.contains(&row.center()))
                );
                let inactive = w
                    .find(if hovered {
                        "command-option-0"
                    } else {
                        "command-option-1"
                    })
                    .bounds();
                let inactive = w.pixel_snap_bounds(inactive).scale(w.scale_factor());
                assert!(!w.painted_quads().iter().any(|q| {
                    q.background == wash.into() && q.bounds.contains(&inactive.center())
                }));
                assert!(w.painted_quads().iter().any(|q| {
                    q.background == rgb(support::tokens(dark).accent).into()
                        && q.bounds.left() == row.left()
                        && q.bounds.size.width == px(2.).scale(w.scale_factor())
                }));
            });
        }
    }
}

#[test]
fn root_popup_overlay_geometry_clamping_reveal_wheel_and_hover_no_scroll() {
    let catalog: Vec<_> = (0..70)
        .map(|i| option(&format!("scroll-command-{i}"), CommandSource::Prompt))
        .collect();
    for (top, fill, below) in [(430., false, false), (40., true, true)] {
        let (_app, mut window, composer) = fixture(top, fill, catalog.clone());
        let editor_before = window.update(|_, w, _| w.find("editor").bounds());
        window.simulate_input("/");
        window.draw();
        window.update(|_, w, _| {
            assert_eq!(w.find("editor").bounds(), editor_before);
            let popup = w.find("command-menu").bounds();
            assert_eq!(popup.size.width, px(540.));
            let first_row = w.find("command-option-0").bounds();
            assert_eq!(first_row.size.height, px(40.5));
            let painted_row = w.pixel_snap_bounds(first_row).scale(w.scale_factor());
            assert!(w.painted_quads().iter().any(|quad| {
                quad.bounds == painted_row && quad.background == rgb(0x6b7d8f).opacity(0.1).into()
            }));
            assert!(popup.size.height <= px(300.));
            assert!(popup.top() >= px(40.) && popup.bottom() <= px(600.));
            if below {
                assert_eq!(popup.top(), editor_before.top() + px(28.));
            } else {
                assert!((popup.bottom() - (editor_before.top() - px(5.))).abs() < px(1.));
            }
            assert!(
                w.painted_quads()
                    .iter()
                    .any(|q| q.background == rgb(support::tokens(false).panel_raised).into())
            );
        });
        window.simulate_keystroke("up");
        paint(&mut window);
        assert_eq!(
            active(&window, &composer).as_deref(),
            Some("scroll-command-0")
        );
        for _ in 0..70 {
            window.simulate_keystroke("down");
        }
        paint(&mut window);
        assert_eq!(
            active(&window, &composer).as_deref(),
            Some("scroll-command-69")
        );
        window.update(|_, w, _| {
            let popup = w.find("command-menu").bounds();
            let row = w.find("command-option-69").bounds();
            assert!(row.top() >= popup.top() && row.bottom() <= popup.bottom());
        });
        window.simulate_keystroke("down");
        paint(&mut window);
        assert_eq!(
            active(&window, &composer).as_deref(),
            Some("scroll-command-69")
        );
        for _ in 0..70 {
            window.simulate_keystroke("up");
        }
        paint(&mut window);
        assert_eq!(
            active(&window, &composer).as_deref(),
            Some("scroll-command-0")
        );
        let position = window.update(|_, w, _| w.find("command-menu").bounds().center());
        let before_wheel = window.read(|_, cx| composer.read(cx).commands.scroll.offset());
        window.simulate_scroll(position, gpui_kit::point(px(0.), px(-30.)));
        window.draw();
        let before = window.read(|_, cx| composer.read(cx).commands.scroll.offset());
        assert_ne!(before.y, before_wheel.y);
        let row_position = window.update(|_, w, _| w.find("command-option-2").bounds().center());
        window.simulate_mouse_move(row_position);
        window.draw();
        assert_eq!(
            window.read(|_, cx| composer.read(cx).commands.scroll.offset()),
            before
        );
        assert_focus(&mut window, &composer);
        set_draft(&mut window, &composer, "/scroll-command-17");
        set_draft(&mut window, &composer, "/");
        window.update(|_, w, _| {
            let popup = w.find("command-menu").bounds();
            let row = w.find("command-option-0").bounds();
            assert!(row.top() >= popup.top() && row.bottom() <= popup.bottom());
        });
    }
}
fn painted_command_thumb(
    window: &mut TestAppWindow<Root>,
    dark: bool,
    hovered: bool,
) -> Bounds<Pixels> {
    window.update(|_, w, _| {
        let popup = w.find("command-menu").bounds().scale(w.scale_factor());
        let ink = rgba((support::tokens(dark).text << 8) | if hovered { 0x8c } else { 0x59 });
        let thumb = w
            .painted_quads()
            .into_iter()
            .find(|q| q.background == ink.into() && popup.contains(&q.bounds.center()))
            .expect("overflowing command menu must paint its shared thumb");
        let scale = w.scale_factor();
        Bounds::new(
            gpui_kit::point(
                px(thumb.bounds.left().0 / scale),
                px(thumb.bounds.top().0 / scale),
            ),
            gpui_kit::size(
                px(thumb.bounds.size.width.0 / scale),
                px(thumb.bounds.size.height.0 / scale),
            ),
        )
    })
}

#[test]
fn root_keyboard_reveal_finishes_at_scroll_limit_and_leaves_wheel_scrolling_in_control() {
    let options: Vec<_> = (0..70)
        .map(|i| option(&format!("command-{i}"), CommandSource::Prompt))
        .collect();
    for dark in [false, true] {
        for (top, fill) in [(430., false), (40., true)] {
            let (_app, mut window, composer) = fixture(top, fill, options.clone());
            window.update(|_, w, cx| composer.update(cx, |s, cx| s.set_theme(dark, w, cx)));
            set_draft(&mut window, &composer, "/");
            for _ in 0..70 {
                window.simulate_keystroke("down");
            }
            paint(&mut window);
            let bottom = window.read(|_, cx| {
                let commands = &composer.read(cx).commands;
                assert!(
                    !commands.reveal.get(),
                    "terminal-row reveal must be consumed"
                );
                assert_eq!(commands.scroll.offset().y, -commands.scroll.max_offset().y);
                commands.scroll.offset()
            });
            let last = painted_command_thumb(&mut window, dark, false);
            let popup = window.update(|_, w, _| w.find("command-menu").bounds());
            window.simulate_scroll(popup.center(), gpui_kit::point(px(0.), px(200.)));
            paint(&mut window);
            let wheeled = window.read(|_, cx| composer.read(cx).commands.scroll.offset());
            assert!(wheeled.y > bottom.y);
            assert!(painted_command_thumb(&mut window, dark, false).top() < last.top());
            paint(&mut window);
            assert_eq!(
                window.read(|_, cx| composer.read(cx).commands.scroll.offset()),
                wheeled,
                "a later frame must not reveal the old keyboard cursor again"
            );
            assert_eq!(active(&window, &composer).as_deref(), Some("command-69"));
            assert_eq!(draft(&window, &composer), "/");
            assert_focus(&mut window, &composer);
        }
    }
}

#[test]
fn root_shared_command_thumb_tracks_wheel_keyboard_hover_and_drag_in_both_themes() {
    let options: Vec<_> = (0..70)
        .map(|i| option(&format!("command-{i}"), CommandSource::Prompt))
        .collect();
    for dark in [false, true] {
        for (top, fill) in [(430., false), (40., true)] {
            let (_app, mut window, composer) = fixture(top, fill, options.clone());
            window.update(|_, w, cx| composer.update(cx, |s, cx| s.set_theme(dark, w, cx)));
            set_draft(&mut window, &composer, "/");
            let initial = painted_command_thumb(&mut window, dark, false);
            let popup = window.update(|_, w, _| w.find("command-menu").bounds());
            window.simulate_scroll(popup.center(), gpui_kit::point(px(0.), px(-200.)));
            paint(&mut window);
            let wheeled = painted_command_thumb(&mut window, dark, false);
            assert!(wheeled.top() > initial.top());
            for _ in 0..70 {
                window.simulate_keystroke("down");
            }
            paint(&mut window);
            let last = painted_command_thumb(&mut window, dark, false);
            assert!(last.top() > wheeled.top());
            window.update(|_, w, _| {
                let row = w.find("command-option-69").bounds();
                assert!(row.top() >= popup.top() && row.bottom() <= popup.bottom());
            });
            window.simulate_mouse_move(last.center());
            paint(&mut window);
            let hovered = painted_command_thumb(&mut window, dark, true);
            let start = hovered.center();
            let end = gpui_kit::point(start.x, popup.top() + px(30.));
            let before_drag = window.read(|_, cx| composer.read(cx).commands.scroll.offset());
            window.simulate_mouse_down(start, MouseButton::Left);
            window.simulate_event(gpui_kit::MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            });
            let moved = window.read(|_, cx| composer.read(cx).commands.scroll.offset());
            assert!(
                moved.y > before_drag.y,
                "held-pointer motion must scroll immediately"
            );
            window.simulate_mouse_up(end, MouseButton::Left);
            window.simulate_mouse_move(gpui_kit::point(popup.left() - px(5.), popup.top()));
            paint(&mut window);
            let dragged = painted_command_thumb(&mut window, dark, false);
            assert!(dragged.top() < last.top());
            assert_eq!(draft(&window, &composer), "/");
            assert_focus(&mut window, &composer);
        }
    }
}

#[test]
fn root_natural_command_surface_has_symmetric_padding_for_zero_one_and_many_results() {
    for dark in [false, true] {
        for (top, fill, below) in [(430., false, false), (40., true, true)] {
            for description in [None, Some(""), Some("A command description")] {
                for count in [0, 1, 3, 70] {
                    let options = (0..count)
                        .map(|i| CommandOption {
                            name: format!("command-{i}"),
                            description: description.map(str::to_owned),
                            source: CommandSource::Prompt,
                            source_info: None,
                        })
                        .collect();
                    let (_app, mut window, composer) = fixture(top, fill, options);
                    window.update(|_, w, cx| composer.update(cx, |s, cx| s.set_theme(dark, w, cx)));
                    set_draft(&mut window, &composer, "/");
                    window.update(|_, w, cx| {
                        let state = composer.read(cx);
                        assert_eq!(state.command_menu_active(), count > 0);
                        if count == 0 {
                            assert!(
                                gpui_kit::base::test_support::find(w, &[], &"command-menu".into())
                                    .is_none()
                            );
                            return;
                        }
                        let popup = w.find("command-menu").bounds();
                        let row_height = if description.is_some_and(|s| !s.is_empty()) {
                            px(40.5)
                        } else {
                            px(24.)
                        };
                        let natural = px(18.) + row_height * count as f32;
                        assert!((popup.size.height - natural.min(px(300.))).abs() < px(0.1));
                        let first = w.find("command-option-0").bounds();
                        assert_eq!(first.size.height, row_height);
                        assert_eq!(first.size.width, popup.size.width - px(18.));
                        assert!(
                            (first.top() - popup.top() - px(9.)).abs() < px(0.1),
                            "count={count}, description={description:?}, below={below}, dark={dark}: popup={popup:?}, first={first:?}, offset={:?}",
                            state.commands.scroll.offset(),
                        );
                        let copy = w.find("command-copy-0").bounds();
                        let source = w.find("command-source-0").bounds();
                        assert!((copy.top() - first.top() - px(5.)).abs() < px(0.1));
                        assert!((first.bottom() - copy.bottom() - px(6.)).abs() < px(0.1));
                        assert!((source.bottom() - copy.bottom()).abs() < px(0.1));
                        assert!(source.left() >= first.left() && source.right() <= first.right());
                        let painted_popup = w.pixel_snap_bounds(popup).scale(w.scale_factor());
                        let thumb_ink = rgba((support::tokens(dark).text << 8) | 0x59);
                        let thumb = w.painted_quads().iter().any(|q| {
                            q.background == thumb_ink.into()
                                && painted_popup.contains(&q.bounds.center())
                        });
                        assert_eq!(
                            thumb,
                            count > 3,
                            "only an overflowing menu paints a thumb"
                        );
                        if count <= 3 {
                            let last = w.find(format!("command-option-{}", count - 1)).bounds();
                            assert!((popup.bottom() - last.bottom() - px(9.)).abs() < px(0.1));
                            assert_eq!(state.commands.scroll.max_offset().y, px(0.));
                        } else {
                            assert!(state.commands.scroll.max_offset().y > px(0.));
                        }
                        let editor = w.find("editor").bounds();
                        if below {
                            assert_eq!(popup.top(), editor.top() + px(28.));
                        } else {
                            assert!((popup.bottom() - editor.top() + px(5.)).abs() < px(1.));
                        }
                        assert!(w.painted_quads().iter().any(|q| {
                            q.background == rgb(support::tokens(dark).panel_raised).into()
                        }));
                    });
                }
            }
        }
    }
}

#[test]
fn root_pointer_selection_submits_without_blur_or_selection_dropthrough() {
    let (_app, mut window, composer) = fixture(40., true, catalog());
    let (received, _subscription) = events(&mut window, &composer);
    window.simulate_input("/");
    window.draw();
    let position = window.update(|_, w, _| w.find("command-option-1").bounds().center());
    window.simulate_mouse_move(position);
    window.draw();
    assert_eq!(active(&window, &composer).as_deref(), Some("summarize"));
    assert_eq!(draft(&window, &composer), "/");
    assert_focus(&mut window, &composer);
    window.simulate_click(position, MouseButton::Left);
    window.draw();
    assert_eq!(*received.borrow(), vec![Event::Send("/summarize".into())]);
    assert_eq!(draft(&window, &composer), "");
    assert_focus(&mut window, &composer);
}
#[test]
fn root_base_enter_action_and_remount_handlers_have_the_same_completion_path() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    let (received, _subscription) = events(&mut window, &composer);
    for reattach in [false, true] {
        window.update(|_, w, cx| {
            composer.update(cx, |s, cx| {
                if reattach {
                    s.reattach_window(w, cx);
                }
                s.editor.update(cx, |e, cx| e.focus(w, cx));
            })
        });
        set_draft(&mut window, &composer, "/sc");
        window.update(|_, w, cx| {
            let focus = composer.read(cx).editor.read(cx).focus_handle(cx);
            focus.dispatch_action(
                &gpui_kit::base::input::Enter {
                    shift: false,
                    secondary: true,
                },
                w,
                cx,
            );
        });
        window.draw();
        assert_eq!(
            *received.borrow(),
            vec![Event::FollowUp("/skill:source-check".into())]
        );
        received.borrow_mut().clear();
        window.update(|_, _, cx| {
            composer.update(cx, |s, cx| {
                s.set_readiness(true, false, false, false, false, cx)
            })
        });
        assert_eq!(draft(&window, &composer), "");
    }
}

#[test]
fn root_command_catalog_updates_disabled_compacting_and_literal_submission() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    let (received, _subscription) = events(&mut window, &composer);
    set_draft(&mut window, &composer, "/");
    window.simulate_keystroke("down");
    window.draw();
    window.update(|_, _, cx| {
        composer.update(cx, |s, cx| {
            s.set_commands(
                vec![
                    option("same", CommandSource::Skill),
                    option("same", CommandSource::Prompt),
                ],
                cx,
            );
            assert_eq!(s.active_command().unwrap().source, CommandSource::Skill);
            s.set_status(true, false, cx);
        })
    });
    window.draw();
    assert!(active(&window, &composer).is_none());
    assert_eq!(draft(&window, &composer), "/");
    window.update(|_, _, cx| {
        composer.update(cx, |s, cx| {
            s.set_status(false, false, cx);
            s.set_commands(catalog(), cx);
            s.set_admission(false, true, cx);
        })
    });
    set_draft(&mut window, &composer, "/sum");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(draft(&window, &composer), "/summarize");
    assert!(received.borrow().is_empty());
    window.update(|_, _, cx| composer.update(cx, |s, cx| s.set_admission(false, false, cx)));
    set_draft(&mut window, &composer, "/ssn");
    window.simulate_keystroke("escape");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(*received.borrow(), vec![Event::Send("/ssn".into())]);
}

#[test]
fn root_resized_empty_fill_popup_stays_in_the_actual_viewport() {
    let catalog = (0..30)
        .map(|i| {
            option(
                &format!("command-with-a-very-long-name-that-must-truncate-{i}"),
                CommandSource::Extension,
            )
        })
        .collect();
    let (_app, mut window, composer) = fixture(40., true, catalog);
    window.simulate_input("/");
    window.draw();
    window.simulate_resize(gpui_kit::size(px(320.), px(180.)));
    window.update(|_, w, cx| w.bounds_changed(cx));
    paint(&mut window);
    window.update(|_, w, _| {
        let popup = w.find("command-menu").bounds();
        assert!(popup.left() >= px(0.) && popup.right() <= px(320.));
        assert!(popup.top() >= px(40.) && popup.bottom() <= px(180.));
        let row = w.find("command-option-0").bounds();
        let copy = w.find("command-copy-0").bounds();
        let source = w.find("command-source-0").bounds();
        assert!(copy.left() >= row.left() && copy.right() <= source.left());
        assert!(source.right() <= row.right() && row.right() <= popup.right());
    });
    assert_eq!(draft(&window, &composer), "/");
    assert_focus(&mut window, &composer);
}

#[test]
fn root_ime_marked_text_is_owned_by_retained_editor() {
    let (_app, mut window, composer) = fixture(430., false, catalog());
    let (received, _subscription) = events(&mut window, &composer);
    set_draft(&mut window, &composer, "/sum");
    window.update(|_, w, cx| {
        composer.read(cx).editor().update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(Some(0..4), "/sum", Some(4..4), w, cx);
        })
    });
    window.simulate_keystroke("enter");
    window.draw();
    assert!(received.borrow().is_empty());
    assert_eq!(draft(&window, &composer), "/sum");
    window.update(|_, w, cx| {
        composer.read(cx).editor().update(cx, |e, cx| {
            assert!(e.marked_text_range(w, cx).is_some());
        })
    });
    window.simulate_keystroke("escape");
    window.draw();
    assert_eq!(active(&window, &composer).as_deref(), Some("summarize"));
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(*received.borrow(), vec![Event::Send("/summarize".into())]);
}
